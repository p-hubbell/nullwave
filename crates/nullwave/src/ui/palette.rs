//! Command palette (⌘⇧P) and quick open (⌘P): one overlay, top-centre, 600 px wide.
//! A query starting with `>` lists commands; anything else lists patterns, instruments
//! and samples. ↑/↓ move, Enter runs, Esc (or a click outside) closes.

use gpui::{
    AnyElement, Context, Entity, FocusHandle, Focusable, KeyDownEvent, ScrollHandle, Subscription, Window,
    div, prelude::*, px, rgba,
};
use nw_workspace::{Binding, COMMANDS, PaletteEntry, PaletteItem, TabKind, rank};

use crate::actions;
use crate::app_state::AppState;
use crate::theme;
use crate::ui::components::{TextField, TextFieldEvent};
use crate::ui::shell::Shell;

const WIDTH: f32 = 600.0;
const MAX_RESULTS: usize = 60;
const ROW_HEIGHT: f32 = 26.0;
const LIST_MAX_HEIGHT: f32 = 12.0 * ROW_HEIGHT;

pub struct PaletteState {
    field: Entity<TextField>,
    selected: usize,
    scroll: ScrollHandle,
    /// Focus before the palette opened; restored on close so commands reach that editor.
    previous_focus: Option<FocusHandle>,
    _subscription: Subscription,
}

/// `cmd-shift-p` → `⌘⇧P`, `ctrl-tab` → `⌃Tab`, `space` → `Space`.
pub fn format_keystrokes(keystrokes: &str) -> String {
    keystrokes
        .split(' ')
        .map(|stroke| {
            let mut out = String::new();
            let parts: Vec<&str> = stroke.split('-').collect();
            // A trailing empty part means the key itself is `-` (e.g. `cmd--`).
            let (mods, key) = match parts.split_last() {
                Some((&"", rest)) if !rest.is_empty() => (&rest[..rest.len() - 1], "-"),
                Some((key, mods)) => (mods, *key),
                None => (&parts[..0], ""),
            };
            for m in mods {
                out.push_str(match *m {
                    "ctrl" => "⌃",
                    "alt" => "⌥",
                    "shift" => "⇧",
                    "cmd" => "⌘",
                    "fn" => "fn ",
                    other => other,
                });
            }
            out.push_str(&match key {
                "space" => "Space".to_string(),
                "backspace" => "⌫".to_string(),
                "delete" => "⌦".to_string(),
                "enter" => "↩".to_string(),
                "escape" => "Esc".to_string(),
                "tab" => "Tab".to_string(),
                "left" => "←".to_string(),
                "right" => "→".to_string(),
                "up" => "↑".to_string(),
                "down" => "↓".to_string(),
                k if k.chars().count() == 1 => k.to_uppercase(),
                k => {
                    let mut c = k.chars();
                    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
                }
            });
            out
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The shortcut shown next to a command: its global binding if any, else the first one.
fn shortcut(bindings: &[Binding], command: &str) -> Option<String> {
    let mut matching = bindings.iter().filter(|b| b.command == command);
    let first = matching.clone().next()?;
    let best = matching.find(|b| b.context.is_none()).unwrap_or(first);
    Some(format_keystrokes(&best.keystrokes))
}

/// The entries for `query` and the query to rank them with.
fn entries(shell: &Shell, state: &AppState, query: &str) -> (Vec<PaletteEntry>, String) {
    if let Some(rest) = query.strip_prefix('>') {
        let commands = COMMANDS
            .iter()
            .filter(|c| !matches!(c.id, "workspace.command_palette" | "workspace.quick_open"))
            .map(|c| PaletteEntry { item: PaletteItem::Command(c.id), label: c.title.to_string() })
            .collect();
        return (commands, rest.trim_start().to_string());
    }
    let patterns = state.project.patterns.iter().map(|p| PaletteEntry {
        item: PaletteItem::Pattern(p.id.clone()),
        label: p.name.clone(),
    });
    let instruments = state.project.instruments.iter().map(|i| PaletteEntry {
        item: PaletteItem::Instrument(i.id.clone()),
        label: i.name.clone(),
    });
    let samples = shell.sidebar.sample_entries().map(|e| PaletteEntry {
        item: PaletteItem::Sample(e.path.clone()),
        label: e.name.clone(),
    });
    (patterns.chain(instruments).chain(samples).collect(), query.to_string())
}

fn ranked(shell: &Shell, state: &AppState, query: &str) -> (Vec<PaletteEntry>, Vec<usize>) {
    let (entries, query) = entries(shell, state, query);
    let mut order = rank(&query, &entries);
    order.truncate(MAX_RESULTS);
    (entries, order)
}

impl Shell {
    /// Opens the palette with `initial` in the field (`">"` for command mode).
    pub fn open_palette(&mut self, initial: &str, window: &mut Window, cx: &mut Context<Shell>) {
        if self.state.read(cx).project_dir.is_some() || initial.starts_with('>') {
            self.refresh_search_index(cx);
        }
        let previous_focus = window.focused(cx);
        let field = cx.new(|cx| TextField::new(initial, "Search patterns, instruments and samples (> for commands)", |c| !c.is_control(), window, cx));
        let subscription = cx.subscribe_in(&field, window, Self::on_palette_event);
        window.focus(&field.focus_handle(cx));
        self.palette = Some(PaletteState {
            field,
            selected: 0,
            scroll: ScrollHandle::new(),
            previous_focus,
            _subscription: subscription,
        });
        cx.notify();
    }

    /// Closes the palette and gives focus back to where it was.
    pub fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Shell>) {
        if let Some(palette) = self.palette.take() {
            match palette.previous_focus {
                Some(focus) => window.focus(&focus),
                None => window.focus(&self.focus_handle(cx)),
            }
        }
        cx.notify();
    }

    fn on_palette_event(
        &mut self,
        _: &Entity<TextField>,
        event: &TextFieldEvent,
        window: &mut Window,
        cx: &mut Context<Shell>,
    ) {
        match event {
            TextFieldEvent::Changed => {
                if let Some(p) = &mut self.palette {
                    p.selected = 0;
                    p.scroll.scroll_to_item(0);
                }
                cx.notify();
            }
            TextFieldEvent::Submit => {
                let index = self.palette.as_ref().map(|p| p.selected);
                if let Some(index) = index {
                    self.run_palette_entry(index, window, cx);
                }
            }
            TextFieldEvent::Cancel => self.close_palette(window, cx),
            // Clicking a row blurs the field first; let the click run the entry.
            TextFieldEvent::Blur => {}
        }
    }

    fn on_palette_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Shell>) {
        let delta: i64 = match event.keystroke.key.as_str() {
            "up" => -1,
            "down" => 1,
            _ => return,
        };
        let Some(palette) = &self.palette else { return };
        let query = palette.field.read(cx).text().to_string();
        let count = ranked(self, self.state.read(cx), &query).1.len();
        if let Some(p) = &mut self.palette
            && count > 0
        {
            p.selected = (p.selected as i64 + delta).rem_euclid(count as i64) as usize;
            p.scroll.scroll_to_item(p.selected);
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Runs the `index`-th visible entry and closes the palette.
    fn run_palette_entry(&mut self, index: usize, window: &mut Window, cx: &mut Context<Shell>) {
        let Some(palette) = &self.palette else { return };
        let query = palette.field.read(cx).text().to_string();
        let (entries, order) = ranked(self, self.state.read(cx), &query);
        let item = order.get(index).map(|&i| entries[i].item.clone());
        self.close_palette(window, cx);
        match item {
            Some(PaletteItem::Command(id)) => {
                if let Some(action) = actions::action_for(id) {
                    window.dispatch_action(action, cx);
                }
            }
            Some(PaletteItem::Pattern(id)) => self.state.update(cx, |s, cx| s.open_tab(TabKind::Pattern(id), cx)),
            Some(PaletteItem::Instrument(id)) => {
                self.state.update(cx, |s, cx| s.open_tab(TabKind::Instrument(id), cx))
            }
            Some(PaletteItem::Sample(path)) => self.state.update(cx, |s, cx| s.preview_sample(path, cx)),
            None => {}
        }
    }
}

pub fn render(shell: &Shell, cx: &Context<Shell>) -> Option<AnyElement> {
    let palette = shell.palette.as_ref()?;
    let state = shell.state.read(cx);
    let query = palette.field.read(cx).text().to_string();
    let command_mode = query.starts_with('>');
    let (entries, order) = ranked(shell, state, &query);
    let selected = palette.selected.min(order.len().saturating_sub(1));

    let rows = order.iter().enumerate().map(|(n, &i)| {
        let entry = &entries[i];
        let hint = match &entry.item {
            PaletteItem::Command(id) => shortcut(&state.bindings, id).unwrap_or_default(),
            PaletteItem::Pattern(_) => "pattern".into(),
            PaletteItem::Instrument(_) => "instrument".into(),
            PaletteItem::Sample(path) => path
                .parent()
                .and_then(|p| p.file_name())
                .map_or_else(|| "sample".into(), |d| format!("sample · {}", d.to_string_lossy())),
        };
        div()
            .id(("palette-row", n))
            .h(px(ROW_HEIGHT))
            .flex_none()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .px(px(10.))
            .cursor_pointer()
            .when(n == selected, |d| d.bg(theme::selection_bg()).text_color(theme::text_strong()))
            .when(n != selected, |d| d.hover(|s| s.bg(theme::hover_bg())))
            .on_click(cx.listener(move |shell, _, window, cx| shell.run_palette_entry(n, window, cx)))
            .child(div().flex_1().min_w_0().overflow_hidden().whitespace_nowrap().child(entry.label.clone()))
            .child(
                div()
                    .flex_none()
                    .text_size(px(theme::SMALL_FONT_SIZE))
                    .text_color(theme::text_muted())
                    .child(hint),
            )
    });
    let empty = order.is_empty().then(|| {
        div()
            .px(px(10.))
            .py(px(6.))
            .text_color(theme::text_faint())
            .child(if command_mode { "No matching commands" } else { "No matching patterns, instruments or samples" })
    });

    Some(
        div()
            .id("palette-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .on_mouse_down(gpui::MouseButton::Left, cx.listener(|shell, _, window, cx| shell.close_palette(window, cx)))
            .flex()
            .justify_center()
            .pt(px(8.))
            .child(
                div()
                    .id("palette")
                    .on_key_down(cx.listener(Shell::on_palette_key))
                    // Clicks inside the palette must not reach the overlay's close handler.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .w(px(WIDTH))
                    .max_h(px(LIST_MAX_HEIGHT + 48.))
                    .flex()
                    .flex_col()
                    .rounded(px(6.))
                    .bg(theme::sidebar_bg())
                    .border_1()
                    .border_color(theme::border())
                    .shadow_lg()
                    .child(div().p(px(8.)).child(palette.field.clone()))
                    .child(
                        div()
                            .id("palette-list")
                            .max_h(px(LIST_MAX_HEIGHT))
                            .overflow_y_scroll()
                            .track_scroll(&palette.scroll)
                            .pb(px(6.))
                            .border_t_1()
                            .border_color(rgba(0xffffff10))
                            .children(rows)
                            .children(empty),
                    ),
            )
            .into_any_element(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_mac_shortcuts() {
        assert_eq!(format_keystrokes("cmd-shift-p"), "⌘⇧P");
        assert_eq!(format_keystrokes("ctrl-tab"), "⌃Tab");
        assert_eq!(format_keystrokes("cmd-\\"), "⌘\\");
        assert_eq!(format_keystrokes("space"), "Space");
        assert_eq!(format_keystrokes("shift-up"), "⇧↑");
        assert_eq!(format_keystrokes("cmd--"), "⌘-");
        assert_eq!(format_keystrokes("cmd-k cmd-s"), "⌘K ⌘S");
    }

    #[test]
    fn shortcut_prefers_global_binding() {
        let b = |k: &str, c: &str, ctx: Option<&str>| Binding {
            keystrokes: k.into(),
            command: c.into(),
            context: ctx.map(String::from),
        };
        let bindings = vec![b("backspace", "x.delete", Some("PianoRoll")), b("cmd-backspace", "x.delete", None)];
        assert_eq!(shortcut(&bindings, "x.delete").as_deref(), Some("⌘⌫"));
        assert_eq!(shortcut(&bindings[..1], "x.delete").as_deref(), Some("⌫"));
        assert_eq!(shortcut(&bindings, "y.other"), None);
    }
}
