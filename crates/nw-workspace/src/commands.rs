use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandInfo {
    pub id: &'static str,
    pub title: &'static str,
}

pub const COMMANDS: &[CommandInfo] = &[
    CommandInfo { id: "workspace.command_palette", title: "Command Palette" },
    CommandInfo { id: "workspace.quick_open", title: "Quick Open" },
    CommandInfo { id: "workspace.toggle_sidebar", title: "Toggle Sidebar" },
    CommandInfo { id: "workspace.toggle_panel", title: "Toggle Panel" },
    CommandInfo { id: "workspace.close_tab", title: "Close Tab" },
    CommandInfo { id: "workspace.next_tab", title: "Next Tab" },
    CommandInfo { id: "workspace.prev_tab", title: "Previous Tab" },
    CommandInfo { id: "workspace.split_right", title: "Split Right" },
    CommandInfo { id: "workspace.show_project", title: "Show Project" },
    CommandInfo { id: "workspace.show_sounds", title: "Show Sounds" },
    CommandInfo { id: "workspace.show_search", title: "Show Search" },
    CommandInfo { id: "project.new", title: "New Project" },
    CommandInfo { id: "project.open", title: "Open Project" },
    CommandInfo { id: "project.save", title: "Save Project" },
    CommandInfo { id: "project.new_pattern", title: "New Pattern" },
    CommandInfo { id: "project.add_sample_folder", title: "Add Sample Folder" },
    CommandInfo { id: "project.open_song", title: "Open Song" },
    CommandInfo { id: "pattern.duplicate", title: "Duplicate Pattern" },
    CommandInfo { id: "pattern.rename", title: "Rename Pattern" },
    CommandInfo { id: "pattern.delete", title: "Delete Pattern" },
    CommandInfo { id: "instrument.delete", title: "Delete Instrument" },
    CommandInfo { id: "edit.undo", title: "Undo" },
    CommandInfo { id: "edit.redo", title: "Redo" },
    CommandInfo { id: "transport.play_stop", title: "Play / Stop" },
    CommandInfo { id: "timeline.duplicate", title: "Duplicate" },
    CommandInfo { id: "timeline.delete", title: "Delete" },
    CommandInfo { id: "timeline.make_unique", title: "Make Unique" },
    CommandInfo { id: "timeline.close_roll", title: "Close Piano Roll" },
    CommandInfo { id: "piano_roll.delete", title: "Delete Notes" },
    CommandInfo { id: "piano_roll.select_all", title: "Select All Notes" },
    CommandInfo { id: "piano_roll.duplicate", title: "Duplicate Notes" },
    CommandInfo { id: "piano_roll.nudge_left", title: "Nudge Notes Left" },
    CommandInfo { id: "piano_roll.nudge_right", title: "Nudge Notes Right" },
    CommandInfo { id: "piano_roll.nudge_up", title: "Nudge Notes Up" },
    CommandInfo { id: "piano_roll.nudge_down", title: "Nudge Notes Down" },
    CommandInfo { id: "piano_roll.octave_up", title: "Octave Up" },
    CommandInfo { id: "piano_roll.octave_down", title: "Octave Down" },
    CommandInfo { id: "app.quit", title: "Quit" },
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub keystrokes: String,
    pub command: String,
    pub context: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeymapError {
    UnknownCommand(String),
    Parse(String),
}

impl std::fmt::Display for KeymapError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeymapError::UnknownCommand(c) => write!(f, "unknown command: {c}"),
            KeymapError::Parse(e) => write!(f, "keymap parse error: {e}"),
        }
    }
}

impl std::error::Error for KeymapError {}

/// Default key bindings. Keystroke strings use GPUI's format.
///
/// Context names: `Workspace` (the root), `PianoRoll` (a focused piano roll),
/// `TextInput` (any text field). `None` means the binding applies everywhere.
pub fn default_bindings() -> Vec<Binding> {
    let b = |k: &str, c: &str, ctx: Option<&str>| Binding {
        keystrokes: k.into(),
        command: c.into(),
        context: ctx.map(String::from),
    };
    vec![
        b("cmd-shift-p", "workspace.command_palette", None),
        b("cmd-p", "workspace.quick_open", None),
        b("cmd-b", "workspace.toggle_sidebar", None),
        b("cmd-j", "workspace.toggle_panel", None),
        b("cmd-w", "workspace.close_tab", None),
        b("ctrl-tab", "workspace.next_tab", None),
        b("ctrl-shift-tab", "workspace.prev_tab", None),
        b("cmd-\\", "workspace.split_right", None),
        b("cmd-shift-e", "workspace.show_project", None),
        b("cmd-shift-s", "workspace.show_sounds", None),
        b("cmd-shift-f", "workspace.show_search", None),
        b("cmd-o", "project.open", None),
        b("cmd-s", "project.save", None),
        b("cmd-n", "project.new_pattern", None),
        b("cmd-z", "edit.undo", None),
        b("cmd-shift-z", "edit.redo", None),
        b("space", "transport.play_stop", Some("Workspace && !TextInput")),
        b("cmd-d", "timeline.duplicate", Some("Timeline")),
        b("backspace", "timeline.delete", Some("Timeline")),
        b("escape", "timeline.close_roll", Some("Timeline")),
        b("backspace", "piano_roll.delete", Some("PianoRoll")),
        b("cmd-a", "piano_roll.select_all", Some("PianoRoll")),
        b("cmd-d", "piano_roll.duplicate", Some("PianoRoll")),
        b("left", "piano_roll.nudge_left", Some("PianoRoll")),
        b("right", "piano_roll.nudge_right", Some("PianoRoll")),
        b("up", "piano_roll.nudge_up", Some("PianoRoll")),
        b("down", "piano_roll.nudge_down", Some("PianoRoll")),
        b("shift-up", "piano_roll.octave_up", Some("PianoRoll")),
        b("shift-down", "piano_roll.octave_down", Some("PianoRoll")),
        b("cmd-q", "app.quit", None),
    ]
}

/// Merge a user RON `Vec<Binding>` over `defaults`. A user entry replaces any binding with the
/// same `(keystrokes, context)`; an empty `command` unbinds.
pub fn merge_user_keymap(
    defaults: Vec<Binding>,
    user_ron: &str,
) -> Result<Vec<Binding>, KeymapError> {
    let user: Vec<Binding> =
        ron::from_str(user_ron).map_err(|e| KeymapError::Parse(e.to_string()))?;
    let mut out = defaults;
    for u in user {
        if !u.command.is_empty() && !COMMANDS.iter().any(|c| c.id == u.command) {
            return Err(KeymapError::UnknownCommand(u.command));
        }
        out.retain(|d| !(d.keystrokes == u.keystrokes && d.context == u.context));
        if !u.command.is_empty() {
            out.push(u);
        }
    }
    Ok(out)
}
