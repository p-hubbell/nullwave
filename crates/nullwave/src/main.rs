//! Nullwave: app start, global allocator, key bindings and the one window.

mod actions;
mod app_state;
mod theme;
mod ui;

use std::rc::Rc;

use gpui::{
    App, AppContext, Application, Bounds, Entity, Focusable, KeyBinding,
    KeyBindingContextPredicate, Menu, MenuItem, TitlebarOptions, WindowBounds, WindowOptions, px,
    size,
};
use nw_workspace::{Binding, default_bindings, merge_user_keymap, settings_dir};

use crate::app_state::AppState;
use crate::ui::shell::Shell;

/// Debug builds abort on any allocation inside an `assert_no_alloc` scope (the audio callback).
#[cfg(debug_assertions)]
#[global_allocator]
static A: assert_no_alloc::AllocDisabler = assert_no_alloc::AllocDisabler;

fn main() {
    Application::new().run(|cx: &mut App| {
        let state = cx.new(AppState::new);
        install_key_bindings(&state, cx);
        let quit_state = state.clone();
        cx.on_action(move |_: &actions::app::Quit, cx| {
            quit_state.update(cx, |s, cx| s.request_quit(cx));
        });
        cx.set_menus(vec![Menu {
            name: "Nullwave".into(),
            items: vec![MenuItem::action("Quit Nullwave", actions::app::Quit)],
        }]);
        // One window: closing it quits.
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1280.), px(800.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions { title: Some("Nullwave".into()), ..Default::default() }),
            window_min_size: Some(size(px(640.), px(400.))),
            ..Default::default()
        };
        let window = cx.open_window(options, |window, cx| {
            // Closing the window goes through the same unsaved-changes prompt as ⌘Q.
            let close_state = state.clone();
            window.on_window_should_close(cx, move |_, cx| {
                close_state.update(cx, |s, cx| s.request_quit(cx));
                false
            });
            let state = state.clone();
            cx.new(|cx| Shell::new(state, window, cx))
        });
        match window {
            Ok(window) => {
                let focused = window.update(cx, |shell, window, cx| {
                    window.focus(&shell.focus_handle(cx));
                    // A prompt from launch (e.g. crash recovery) takes the focus.
                    shell.sync_modal_focus(window, cx);
                    cx.activate(true);
                });
                if let Err(e) = focused {
                    eprintln!("nullwave: could not focus the window: {e}");
                }
            }
            Err(e) => {
                eprintln!("nullwave: could not open the window: {e}");
                cx.quit();
            }
        }
    });
}

/// Binds `merge_user_keymap(default_bindings(), keymap.ron)`. Any problem with the user
/// keymap is logged and the defaults are used instead.
fn install_key_bindings(state: &Entity<AppState>, cx: &mut App) {
    let mut log = Vec::new();
    let defaults = default_bindings();
    let path = settings_dir().join("keymap.ron");
    let merged = match std::fs::read_to_string(&path) {
        Ok(text) => match merge_user_keymap(defaults.clone(), &text) {
            Ok(merged) => Some(merged),
            Err(e) => {
                log.push(format!("{}: {e}; using the default key bindings", path.display()));
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            log.push(format!("Could not read {}: {e}; using the default key bindings", path.display()));
            None
        }
    };
    let user = merged.map(|bindings| (to_key_bindings(&bindings, cx), bindings));
    let (keys, in_effect) = match user {
        Some((Ok(keys), bindings)) => {
            log.push(format!("Key bindings loaded from {}", path.display()));
            (keys, bindings)
        }
        Some((Err(e), _)) => {
            log.push(format!("{}: {e}; using the default key bindings", path.display()));
            (default_key_bindings(&defaults, &mut log, cx), defaults)
        }
        None => (default_key_bindings(&defaults, &mut log, cx), defaults),
    };
    cx.bind_keys(keys);
    state.update(cx, |s, _| {
        s.bindings = in_effect;
        for line in log {
            s.log(line);
        }
    });
}

fn default_key_bindings(defaults: &[Binding], log: &mut Vec<String>, cx: &App) -> Vec<KeyBinding> {
    to_key_bindings(defaults, cx).unwrap_or_else(|e| {
        log.push(format!("Internal error in the default key bindings: {e}"));
        Vec::new()
    })
}

fn to_key_bindings(bindings: &[Binding], cx: &App) -> Result<Vec<KeyBinding>, String> {
    bindings
        .iter()
        .map(|b| {
            let action = actions::action_for(&b.command)
                .ok_or_else(|| format!("unknown command {}", b.command))?;
            let predicate = match b.context.as_deref() {
                Some(context) => Some(Rc::new(
                    KeyBindingContextPredicate::parse(context)
                        .map_err(|e| format!("invalid context \"{context}\": {e}"))?,
                )),
                None => None,
            };
            KeyBinding::load(&b.keystrokes, action, predicate, false, None, cx.keyboard_mapper().as_ref())
                .map_err(|e| format!("invalid keystroke \"{}\": {e}", b.keystrokes))
        })
        .collect()
}
