use nw_workspace::*;

#[test]
fn every_default_binding_names_a_known_command() {
    for b in default_bindings() {
        assert!(COMMANDS.iter().any(|c| c.id == b.command), "{}", b.command);
    }
}

#[test]
fn no_duplicate_keystroke_in_same_context() {
    let b = default_bindings();
    for (i, x) in b.iter().enumerate() {
        for y in &b[i + 1..] {
            assert!(
                !(x.keystrokes == y.keystrokes && x.context == y.context),
                "duplicate {} in {:?}",
                x.keystrokes,
                x.context
            );
        }
    }
}

#[test]
fn user_override_replaces_default() {
    let user = r#"[(keystrokes: "cmd-s", command: "project.open", context: None)]"#;
    let m = merge_user_keymap(default_bindings(), user).unwrap();
    let hits: Vec<_> = m.iter().filter(|b| b.keystrokes == "cmd-s" && b.context.is_none()).collect();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].command, "project.open");
}

#[test]
fn empty_command_unbinds() {
    let user = r#"[(keystrokes: "cmd-q", command: "", context: None)]"#;
    let m = merge_user_keymap(default_bindings(), user).unwrap();
    assert!(!m.iter().any(|b| b.keystrokes == "cmd-q"));
}

#[test]
fn unknown_command_errors() {
    let user = r#"[(keystrokes: "cmd-k", command: "nope.nothing", context: None)]"#;
    assert_eq!(
        merge_user_keymap(default_bindings(), user),
        Err(KeymapError::UnknownCommand("nope.nothing".into()))
    );
}

#[test]
fn bad_ron_errors() {
    assert!(matches!(
        merge_user_keymap(default_bindings(), "not ron ["),
        Err(KeymapError::Parse(_))
    ));
}

fn entry(label: &str) -> PaletteEntry {
    PaletteEntry { item: PaletteItem::Command("x"), label: label.into() }
}

#[test]
fn rank_fuzzy_prefers_tighter_match() {
    let entries = vec![
        entry("Toggle Panel"),
        entry("Open Sidebar Toggle Bits"),
        entry("Toggle Sidebar"),
        entry("Save"),
    ];
    let r = rank("tsb", &entries);
    assert_eq!(r[0], 2);
    assert!(!r.contains(&0) && !r.contains(&3));
}

#[test]
fn empty_query_keeps_order() {
    let entries = vec![entry("b"), entry("a"), entry("c")];
    assert_eq!(rank("", &entries), vec![0, 1, 2]);
}

#[test]
fn timeline_commands_exist_and_bind_in_context() {
    for id in ["project.open_song", "pattern.duplicate", "timeline.duplicate", "timeline.delete", "timeline.make_unique", "timeline.close_roll"] {
        assert!(COMMANDS.iter().any(|c| c.id == id), "{id}");
    }
    let bound = |keys: &str, ctx: &str| {
        default_bindings().into_iter().find(|b| b.keystrokes == keys && b.context.as_deref() == Some(ctx)).map(|b| b.command)
    };
    assert_eq!(bound("cmd-d", "Timeline").as_deref(), Some("timeline.duplicate"));
    assert_eq!(bound("backspace", "Timeline").as_deref(), Some("timeline.delete"));
    assert_eq!(bound("escape", "Timeline").as_deref(), Some("timeline.close_roll"));
}
