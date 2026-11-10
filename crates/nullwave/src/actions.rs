//! One GPUI action per command in `nw_workspace::COMMANDS`.
//!
//! The command id `ns.snake_name` maps to the action `ns::CamelName` (GPUI name
//! `ns::CamelName`), declared in a Rust module named after the namespace.

use gpui::Action;

pub mod workspace {
    gpui::actions!(
        workspace,
        [
            CommandPalette,
            QuickOpen,
            ToggleSidebar,
            TogglePanel,
            CloseTab,
            NextTab,
            PrevTab,
            SplitRight,
            ShowProject,
            ShowSounds,
            ShowSearch,
        ]
    );
}

pub mod project {
    gpui::actions!(project, [New, Open, Save, NewPattern, AddSampleFolder]);
}

pub mod pattern {
    gpui::actions!(pattern, [Rename, Delete]);
}

pub mod instrument {
    gpui::actions!(instrument, [Delete]);
}

pub mod edit {
    gpui::actions!(edit, [Undo, Redo]);
}

pub mod transport {
    gpui::actions!(transport, [PlayStop]);
}

pub mod piano_roll {
    gpui::actions!(
        piano_roll,
        [
            Delete,
            SelectAll,
            Duplicate,
            NudgeLeft,
            NudgeRight,
            NudgeUp,
            NudgeDown,
            OctaveUp,
            OctaveDown,
        ]
    );
}

pub mod app {
    gpui::actions!(app, [Quit]);
}

/// The action for a command id from `COMMANDS`, or `None` for an unknown id.
pub fn action_for(id: &str) -> Option<Box<dyn Action>> {
    let action: Box<dyn Action> = match id {
        "workspace.command_palette" => Box::new(workspace::CommandPalette),
        "workspace.quick_open" => Box::new(workspace::QuickOpen),
        "workspace.toggle_sidebar" => Box::new(workspace::ToggleSidebar),
        "workspace.toggle_panel" => Box::new(workspace::TogglePanel),
        "workspace.close_tab" => Box::new(workspace::CloseTab),
        "workspace.next_tab" => Box::new(workspace::NextTab),
        "workspace.prev_tab" => Box::new(workspace::PrevTab),
        "workspace.split_right" => Box::new(workspace::SplitRight),
        "workspace.show_project" => Box::new(workspace::ShowProject),
        "workspace.show_sounds" => Box::new(workspace::ShowSounds),
        "workspace.show_search" => Box::new(workspace::ShowSearch),
        "project.new" => Box::new(project::New),
        "project.open" => Box::new(project::Open),
        "project.save" => Box::new(project::Save),
        "project.new_pattern" => Box::new(project::NewPattern),
        "project.add_sample_folder" => Box::new(project::AddSampleFolder),
        "pattern.rename" => Box::new(pattern::Rename),
        "pattern.delete" => Box::new(pattern::Delete),
        "instrument.delete" => Box::new(instrument::Delete),
        "edit.undo" => Box::new(edit::Undo),
        "edit.redo" => Box::new(edit::Redo),
        "transport.play_stop" => Box::new(transport::PlayStop),
        "piano_roll.delete" => Box::new(piano_roll::Delete),
        "piano_roll.select_all" => Box::new(piano_roll::SelectAll),
        "piano_roll.duplicate" => Box::new(piano_roll::Duplicate),
        "piano_roll.nudge_left" => Box::new(piano_roll::NudgeLeft),
        "piano_roll.nudge_right" => Box::new(piano_roll::NudgeRight),
        "piano_roll.nudge_up" => Box::new(piano_roll::NudgeUp),
        "piano_roll.nudge_down" => Box::new(piano_roll::NudgeDown),
        "piano_roll.octave_up" => Box::new(piano_roll::OctaveUp),
        "piano_roll.octave_down" => Box::new(piano_roll::OctaveDown),
        "app.quit" => Box::new(app::Quit),
        _ => return None,
    };
    Some(action)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nw_workspace::COMMANDS;

    fn camel(snake: &str) -> String {
        snake
            .split('_')
            .map(|w| {
                let mut c = w.chars();
                c.next().map(|f| f.to_ascii_uppercase().to_string() + c.as_str()).unwrap_or_default()
            })
            .collect()
    }

    #[test]
    fn every_command_has_an_action_with_the_matching_name() {
        for cmd in COMMANDS {
            let action = action_for(cmd.id).unwrap_or_else(|| panic!("no action for {}", cmd.id));
            let (ns, name) = cmd.id.split_once('.').unwrap();
            assert_eq!(action.name(), format!("{ns}::{}", camel(name)), "{}", cmd.id);
        }
    }

    #[test]
    fn unknown_command_has_no_action() {
        assert!(action_for("workspace.nope").is_none());
    }
}
