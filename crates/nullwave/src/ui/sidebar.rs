//! Activity bar and sidebar: Project (project / presets / sample folders), Sounds
//! (sample folders only) and Search (patterns, instruments and sample names).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui::{AnyElement, Context, SharedString, Task, div, prelude::*, px};
use nw_workspace::{
    BrowserEntry, PaletteEntry, PaletteItem, SidebarView, TabKind, list_dir, rank,
};

use crate::app_state::AppState;
use crate::theme;
use crate::ui::components::{dim_line, list_row, section_header};
use crate::ui::shell::Shell;

const MAX_SEARCH_RESULTS: usize = 200;
const MAX_INDEX_DEPTH: usize = 8;
const MAX_INDEXED_FILES: usize = 20_000;

/// Sidebar-local UI state: which sample folders are expanded and their listings.
#[derive(Default)]
pub struct SidebarState {
    expanded: HashSet<PathBuf>,
    listings: HashMap<PathBuf, Result<Vec<BrowserEntry>, String>>,
    /// Audio files under the sample folders, for Search.
    sample_index: Vec<BrowserEntry>,
    index_task: Option<Task<()>>,
}

impl SidebarState {
    /// Expands (listing the folder now) or collapses (forgetting it and everything below).
    fn toggle(&mut self, path: &Path) {
        if self.expanded.remove(path) {
            self.expanded.retain(|p| !p.starts_with(path));
            self.listings.retain(|p, _| !p.starts_with(path));
        } else {
            self.expanded.insert(path.to_path_buf());
            self.listings.insert(path.to_path_buf(), list_dir(path).map_err(|e| e.to_string()));
        }
    }

    /// Re-indexes the sample folders on the background executor.
    pub fn refresh_index(&mut self, folders: Vec<PathBuf>, cx: &mut Context<Shell>) {
        let walk = cx.background_executor().spawn(async move { index_samples(&folders) });
        self.index_task = Some(cx.spawn(async move |this, cx| {
            let files = walk.await;
            this.update(cx, |shell, cx| {
                shell.sidebar.sample_index = files;
                cx.notify();
            })
            .ok();
        }));
    }
}

fn index_samples(folders: &[PathBuf]) -> Vec<BrowserEntry> {
    let mut out = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = folders.iter().map(|f| (f.clone(), 0)).collect();
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = list_dir(&dir) else { continue };
        for entry in entries {
            if entry.is_dir {
                if depth < MAX_INDEX_DEPTH {
                    stack.push((entry.path, depth + 1));
                }
            } else if out.len() < MAX_INDEXED_FILES {
                out.push(entry);
            }
        }
    }
    out
}

pub fn render_activity_bar(shell: &Shell, cx: &Context<Shell>) -> impl IntoElement {
    let ws = &shell.state.read(cx).workspace;
    let icon = |view: SidebarView, glyph: &'static str, id: &'static str| {
        let active = ws.sidebar_visible && ws.sidebar_view == view;
        div()
            .id(id)
            .w(px(theme::ACTIVITY_BAR_WIDTH))
            .h(px(32.))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(15.))
            .cursor_pointer()
            .border_l_2()
            .when(active, |d| d.text_color(theme::text_strong()).border_color(theme::accent()))
            .when(!active, |d| {
                d.text_color(theme::text_muted())
                    .border_color(theme::transparent())
                    .hover(|s| s.text_color(theme::text()))
            })
            .on_click(cx.listener(move |shell, _, window, cx| shell.activity_clicked(view, window, cx)))
            .child(glyph)
    };
    div()
        .w(px(theme::ACTIVITY_BAR_WIDTH))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .pt(px(6.))
        .gap(px(2.))
        .bg(theme::chrome_bg())
        .border_r_1()
        .border_color(theme::border())
        .child(icon(SidebarView::Project, "▤", "activity-project"))
        .child(icon(SidebarView::Sounds, "♫", "activity-sounds"))
        .child(icon(SidebarView::Search, "⌕", "activity-search"))
}

pub fn render(shell: &Shell, width: f32, cx: &Context<Shell>) -> impl IntoElement {
    let state = shell.state.read(cx);
    let mut rows: Vec<AnyElement> = Vec::new();
    match state.workspace.sidebar_view {
        SidebarView::Project => {
            project_section(shell, state, &mut rows);
            rows.push(section_header("presets").into_any_element());
            rows.push(dim_line("available when the mixer arrives", 1).into_any_element());
            sample_folders_section(shell, state, cx, &mut rows);
        }
        SidebarView::Sounds => sample_folders_section(shell, state, cx, &mut rows),
        SidebarView::Search => search_section(shell, state, cx, &mut rows),
    }
    div()
        .id("sidebar")
        .w(px(width))
        .h_full()
        .flex_none()
        .flex()
        .flex_col()
        .pb(px(8.))
        .bg(theme::sidebar_bg())
        .overflow_y_scroll()
        .children(rows)
}

fn project_section(shell: &Shell, state: &AppState, rows: &mut Vec<AnyElement>) {
    let Some(dir) = &state.project_dir else {
        rows.push(section_header("project").into_any_element());
        rows.push(dim_line("No project open", 1).into_any_element());
        return;
    };
    let name = dir.file_name().map_or_else(|| dir.display().to_string(), |n| n.to_string_lossy().into());
    rows.push(section_header(format!("project · {name}")).into_any_element());
    rows.push(dim_line("patterns", 0).text_color(theme::text_muted()).into_any_element());
    let active = state.workspace.active_tab().cloned();
    for pattern in &state.project.patterns {
        let tab = TabKind::Pattern(pattern.id.clone());
        let selected = active.as_ref() == Some(&tab);
        let state_entity = shell.state.clone();
        rows.push(
            list_row(SharedString::from(format!("pattern:{}", pattern.id.0)), 1, selected)
                .on_click(move |_, _, cx| {
                    state_entity.update(cx, |s, cx| s.open_tab(tab.clone(), cx));
                })
                .child(pattern.name.clone())
                .into_any_element(),
        );
    }
    let state_entity = shell.state.clone();
    rows.push(
        list_row("new-pattern", 1, false)
            .text_color(theme::text_muted())
            .on_click(move |_, _, cx| state_entity.update(cx, |s, cx| s.new_pattern(cx)))
            .child("+ new pattern")
            .into_any_element(),
    );
}

fn sample_folders_section(
    shell: &Shell,
    state: &AppState,
    cx: &Context<Shell>,
    rows: &mut Vec<AnyElement>,
) {
    rows.push(section_header("sample folders").into_any_element());
    for folder in &state.settings.sample_folders {
        let label = folder.file_name().map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().into());
        folder_rows(shell, folder, label, 0, cx, rows);
    }
    let state_entity = shell.state.clone();
    rows.push(
        list_row("add-folder", 0, false)
            .text_color(theme::text_muted())
            .on_click(move |_, _, cx| state_entity.update(cx, |s, cx| s.add_sample_folder(cx)))
            .child("+ add folder")
            .into_any_element(),
    );
}

/// A folder row and, when expanded, its listing (recursively).
fn folder_rows(
    shell: &Shell,
    path: &Path,
    label: String,
    depth: usize,
    cx: &Context<Shell>,
    rows: &mut Vec<AnyElement>,
) {
    let expanded = shell.sidebar.expanded.contains(path);
    let toggle_path = path.to_path_buf();
    rows.push(
        list_row(SharedString::from(format!("dir:{}", path.display())), depth, false)
            .on_click(cx.listener(move |shell, _, _, cx| {
                shell.sidebar.toggle(&toggle_path);
                cx.notify();
            }))
            .child(div().w(px(10.)).text_color(theme::text_muted()).child(if expanded { "▾" } else { "▸" }))
            .child(label)
            .into_any_element(),
    );
    if !expanded {
        return;
    }
    match shell.sidebar.listings.get(path) {
        Some(Ok(entries)) if entries.is_empty() => {
            rows.push(dim_line("empty", depth + 2).into_any_element());
        }
        Some(Ok(entries)) => {
            for entry in entries {
                if entry.is_dir {
                    folder_rows(shell, &entry.path, entry.name.clone(), depth + 1, cx, rows);
                } else {
                    rows.push(
                        list_row(SharedString::from(format!("file:{}", entry.path.display())), depth + 2, false)
                            .child(entry.name.clone())
                            .into_any_element(),
                    );
                }
            }
        }
        Some(Err(_)) => rows.push(dim_line("not connected", depth + 2).into_any_element()),
        None => {}
    }
}

fn search_entries(shell: &Shell, state: &AppState) -> Vec<PaletteEntry> {
    let patterns = state.project.patterns.iter().map(|p| PaletteEntry {
        item: PaletteItem::Pattern(p.id.clone()),
        label: p.name.clone(),
    });
    let instruments = state.project.instruments.iter().map(|i| PaletteEntry {
        item: PaletteItem::Instrument(i.id.clone()),
        label: i.name.clone(),
    });
    let samples = shell.sidebar.sample_index.iter().map(|e| PaletteEntry {
        item: PaletteItem::Sample(e.path.clone()),
        label: e.name.clone(),
    });
    patterns.chain(instruments).chain(samples).collect()
}

fn tab_for(item: &PaletteItem) -> Option<TabKind> {
    match item {
        PaletteItem::Pattern(p) => Some(TabKind::Pattern(p.clone())),
        PaletteItem::Instrument(i) => Some(TabKind::Instrument(i.clone())),
        PaletteItem::Command(_) | PaletteItem::Sample(_) => None,
    }
}

fn search_section(shell: &Shell, state: &AppState, cx: &Context<Shell>, rows: &mut Vec<AnyElement>) {
    rows.push(section_header("search").into_any_element());
    rows.push(div().px(px(10.)).pb(px(6.)).child(shell.search_field.clone()).into_any_element());
    let query = shell.search_field.read(cx).text().to_string();
    if query.trim().is_empty() {
        rows.push(dim_line("Type to search patterns, instruments and samples", 0).into_any_element());
        return;
    }
    let entries = search_entries(shell, state);
    let ranked = rank(&query, &entries);
    if ranked.is_empty() {
        rows.push(dim_line("No results", 0).into_any_element());
    }
    for (n, &i) in ranked.iter().take(MAX_SEARCH_RESULTS).enumerate() {
        let entry = &entries[i];
        let kind = match entry.item {
            PaletteItem::Pattern(_) => "pattern",
            PaletteItem::Instrument(_) => "instrument",
            PaletteItem::Sample(_) => "sample",
            PaletteItem::Command(_) => "command",
        };
        let tab = tab_for(&entry.item);
        let state_entity = shell.state.clone();
        rows.push(
            list_row(("search-result", n), 0, false)
                .on_click(move |_, _, cx| {
                    if let Some(tab) = tab.clone() {
                        state_entity.update(cx, |s, cx| s.open_tab(tab, cx));
                    }
                })
                .child(div().flex_1().overflow_hidden().child(entry.label.clone()))
                .child(div().text_size(px(theme::SMALL_FONT_SIZE)).text_color(theme::text_faint()).child(kind))
                .into_any_element(),
        );
    }
}

/// Enter in the search field opens the best match, if it is a pattern or instrument.
pub fn open_first_search_result(shell: &mut Shell, cx: &mut Context<Shell>) {
    let query = shell.search_field.read(cx).text().to_string();
    if query.trim().is_empty() {
        return;
    }
    let state = shell.state.read(cx);
    let entries = search_entries(shell, state);
    let tab = rank(&query, &entries).first().and_then(|&i| tab_for(&entries[i].item));
    if let Some(tab) = tab {
        shell.state.update(cx, |s, cx| s.open_tab(tab, cx));
    }
}
