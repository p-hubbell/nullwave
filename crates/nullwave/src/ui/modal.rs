//! In-window prompts: unsaved changes, recovery, outside changes and errors. They are
//! overlays inside the one window, never native alerts. Enter presses the primary button;
//! Esc dismisses the prompt without doing anything.

use std::path::PathBuf;

use gpui::{AnyElement, Context, KeyDownEvent, Window, div, prelude::*, px, rgba};
use nw_core::Project;

use crate::theme;
use crate::ui::shell::Shell;

/// What to do once unsaved changes are dealt with.
#[derive(Clone, Debug)]
pub enum Next {
    Quit,
    Open(PathBuf),
    Create(PathBuf),
}

#[derive(Clone, Debug)]
pub enum ModalAction {
    /// Close the prompt; nothing else happens.
    Dismiss,
    SaveThen(Next),
    DiscardThen(Next),
    Restore(Box<Project>),
    DiscardRecovery,
    ReloadFromDisk,
    KeepMine,
}

#[derive(Clone, Debug)]
pub struct ModalButton {
    pub label: &'static str,
    pub action: ModalAction,
}

#[derive(Clone, Debug)]
pub struct Modal {
    pub title: String,
    pub message: String,
    pub buttons: Vec<ModalButton>,
    /// Index of the button Enter presses.
    pub primary: usize,
}

impl Modal {
    pub fn error(title: impl Into<String>, message: impl Into<String>) -> Modal {
        Modal {
            title: title.into(),
            message: message.into(),
            buttons: vec![ModalButton { label: "OK", action: ModalAction::Dismiss }],
            primary: 0,
        }
    }

    /// "Save changes before …?" with Save / Don't Save / Cancel.
    pub fn unsaved(next: Next) -> Modal {
        let title = match next {
            Next::Quit => "Save changes before quitting?",
            Next::Open(_) | Next::Create(_) => "Save changes before closing this project?",
        };
        Modal {
            title: title.into(),
            message: "Your changes will be lost if you don't save them.".into(),
            buttons: vec![
                ModalButton { label: "Save", action: ModalAction::SaveThen(next.clone()) },
                ModalButton { label: "Don't Save", action: ModalAction::DiscardThen(next) },
                ModalButton { label: "Cancel", action: ModalAction::Dismiss },
            ],
            primary: 0,
        }
    }

    pub fn recovery(project: Project) -> Modal {
        Modal {
            title: "Restore unsaved changes from last session?".into(),
            message: "Nullwave found changes that were not saved when it last closed.".into(),
            buttons: vec![
                ModalButton { label: "Restore", action: ModalAction::Restore(Box::new(project)) },
                ModalButton { label: "Discard", action: ModalAction::DiscardRecovery },
            ],
            primary: 0,
        }
    }

    pub fn outside_changes(files: &[String]) -> Modal {
        Modal {
            title: "Files changed on disk".into(),
            message: format!(
                "Files changed on disk: {}. You also have unsaved changes.",
                files.join(", ")
            ),
            buttons: vec![
                ModalButton { label: "Reload from disk", action: ModalAction::ReloadFromDisk },
                ModalButton { label: "Keep mine", action: ModalAction::KeepMine },
            ],
            primary: 1,
        }
    }
}

impl Shell {
    fn on_modal_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Shell>) {
        match event.keystroke.key.as_str() {
            "enter" => {
                let primary = self.state.read(cx).modals.front().map(|m| m.primary);
                if let Some(primary) = primary {
                    self.state.update(cx, |s, cx| s.resolve_modal(Some(primary), cx));
                }
            }
            "escape" => self.state.update(cx, |s, cx| s.resolve_modal(None, cx)),
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// The front prompt, if any, over a dimmed window.
pub fn render(shell: &Shell, cx: &Context<Shell>) -> Option<AnyElement> {
    let modal = shell.state.read(cx).modals.front()?;
    let buttons = modal.buttons.iter().enumerate().map(|(i, b)| {
        let primary = i == modal.primary;
        let state = shell.state.clone();
        div()
            .id(("modal-button", i))
            .px(px(14.))
            .py(px(5.))
            .rounded(px(4.))
            .border_1()
            .cursor_pointer()
            .when(primary, |d| {
                d.bg(theme::accent()).border_color(theme::accent()).text_color(rgba(0x0b1220ff))
            })
            .when(!primary, |d| {
                d.bg(theme::input_bg())
                    .border_color(theme::border())
                    .text_color(theme::text_strong())
                    .hover(|s| s.bg(theme::hover_bg()))
            })
            .on_click(move |_, _, cx| state.update(cx, |s, cx| s.resolve_modal(Some(i), cx)))
            .child(b.label)
    });
    Some(
        div()
            .id("modal-overlay")
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x00000073))
            .child(
                div()
                    .key_context("Modal")
                    .track_focus(&shell.modal_focus)
                    .on_key_down(cx.listener(Shell::on_modal_key))
                    .w(px(460.))
                    .p(px(18.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .rounded(px(6.))
                    .bg(theme::sidebar_bg())
                    .border_1()
                    .border_color(theme::border())
                    .child(div().text_size(px(14.)).text_color(theme::text_strong()).child(modal.title.clone()))
                    .child(div().text_color(theme::text()).child(modal.message.clone()))
                    .child(div().pt(px(6.)).flex().justify_end().gap(px(8.)).children(buttons)),
            )
            .into_any_element(),
    )
}
