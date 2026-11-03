//! Host-only reload contract tests, with no Wayland session or provider side effects.
use nuraloumi_shell::config_reload::{prepare_config_reload, prepare_from_file};
use nuraloumi_shell::{
    apply_launcher_preferences, build_family, ApplicationEntry, FixtureSnapshot, MenuFamily,
    ShellConfig, ShellState, Theme,
};
use std::path::Path;

#[test]
fn invalid_file_does_not_mutate_active_config_or_menu_selection() {
    let active = ShellConfig::default();
    let snapshot = FixtureSnapshot::default();
    let shell = ShellState::new(build_family(MenuFamily::Launcher, &snapshot), false).unwrap();
    let original_selection = shell.state.selected_id.clone();
    let original_model = shell.menu.clone();

    let result = prepare_config_reload(
        &active,
        Path::new("config.toml"),
        "theme = 'light'\nrow_height = 1\n",
        false,
    );
    assert_eq!(result.unwrap_err().code, "validation_error");
    assert_eq!(active.theme, Theme::Dark);
    assert_eq!(shell.state.selected_id, original_selection);
    assert_eq!(shell.menu, original_model);
}

#[test]
fn valid_theme_launcher_label_and_behavior_transition_is_staged() {
    let active = ShellConfig::default();
    let mut next = active.clone();
    next.theme = Theme::Light;
    next.reduced_motion = true;
    next.menu_dismissal.after_one_shot_action = false;
    next.launcher
        .labels
        .insert("foot.desktop".into(), "Edited Terminal".into());

    let prepared = prepare_config_reload(
        &active,
        Path::new("config.toml"),
        &toml::to_string(&next).unwrap(),
        false,
    )
    .unwrap();
    let raw = FixtureSnapshot {
        applications: vec![ApplicationEntry {
            id: "foot.desktop".into(),
            label: "Terminal".into(),
            generic_name: None,
            keywords: Vec::new(),
            launchable: true,
            pinned: false,
        }],
        ..FixtureSnapshot::default()
    };
    let mut staged = raw.clone();
    apply_launcher_preferences(&mut staged.applications, &prepared.config.launcher);
    assert_eq!(staged.applications[0].label, "Edited Terminal");
    assert_eq!(raw.applications[0].label, "Terminal");
    let original = ShellState::new(build_family(MenuFamily::Launcher, &raw), false).unwrap();
    let selected_id = original.state.selected_id.clone();
    let mut staged_shell = original.clone();
    staged_shell.reduced_motion = prepared.config.reduced_motion;
    staged_shell
        .refresh_family(MenuFamily::Launcher, &staged)
        .unwrap();
    assert_eq!(staged_shell.state.selected_id, selected_id);
    assert_eq!(original.state.selected_id, selected_id);
    assert!(!original.reduced_motion);
    assert!(staged_shell.reduced_motion);
    assert_eq!(active.theme, Theme::Dark);
    assert!(prepared.changed);
    assert_eq!(prepared.config.theme, Theme::Light);
    assert!(prepared.config.reduced_motion);
    assert!(!prepared.config.menu_dismissal.after_one_shot_action);
}

#[test]
fn rejected_structural_change_leaves_live_state_untouched() {
    let active = ShellConfig::default();
    let mut next = active.clone();
    next.theme = Theme::Light;
    next.panel_height += 1;
    next.providers.terminal = "other-terminal".into();
    let error = prepare_config_reload(
        &active,
        Path::new("config.json"),
        &serde_json::to_string(&next).unwrap(),
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, "restart_required");
    assert!(error.message.contains("panel_height"));
    assert!(error.message.contains("providers"));
    assert_eq!(active, ShellConfig::default());
}

#[test]
fn missing_source_fails_closed() {
    let root =
        std::env::temp_dir().join(format!("nuraloumi-missing-reload-{}", std::process::id()));
    let path = root.join("no-such-file.toml");
    let error = prepare_from_file(&ShellConfig::default(), Some(&path), false).unwrap_err();
    assert_eq!(error.code, "io_error");
}

#[test]
fn oversized_reload_cannot_reach_apply_stage() {
    let source = " ".repeat(1024 * 1024 + 1);
    let error = prepare_config_reload(
        &ShellConfig::default(),
        Path::new("config.toml"),
        &source,
        false,
    )
    .unwrap_err();
    assert_eq!(error.code, "config_too_large");
}
