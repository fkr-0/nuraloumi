//! Launcher integration exercises public state-to-menu and explicit action contracts.
//! All paths are fixture-local; no Wayland session or real home directory is touched.
use nuraloumi_shell::favorites::FavoritesStore;
use nuraloumi_shell::{
    apply_launcher_state, build_launcher_menu_for, record_user_application_activation_at_path,
    update_user_launcher_state_at_path, ApplicationEntry, FixtureSnapshot, LauncherPreferences,
    MenuAction, MenuFamily, OverviewMode, ShellState,
};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

struct FixtureState(PathBuf);
impl FixtureState {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "nuraloumi-launcher-integration-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path.join("launcher-state.json"))
    }
}
impl Drop for FixtureState {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(self.0.parent().unwrap());
    }
}

fn snapshot() -> FixtureSnapshot {
    FixtureSnapshot {
        applications: ["alpha.desktop", "terminal.desktop", "zeta.desktop"]
            .into_iter()
            .map(|id| ApplicationEntry {
                id: id.to_owned(),
                label: id.trim_end_matches(".desktop").to_owned(),
                generic_name: None,
                keywords: Vec::new(),
                launchable: true,
                pinned: false,
            })
            .collect(),
        ..FixtureSnapshot::default()
    }
}

#[test]
fn runtime_action_contract_is_visible_and_only_uses_discovered_desktop_ids() {
    let mut snapshot = snapshot();
    let pref = LauncherPreferences::default();
    let fixture = FixtureState::new();
    let apps = build_launcher_menu_for(&snapshot, OverviewMode::Apps);
    let action = apps
        .items
        .iter()
        .find(|item| item.id == "launcher.pin.alpha.desktop")
        .and_then(|item| item.action.as_ref())
        .expect("visible favorite toggle");
    assert!(matches!(action, MenuAction::Custom { kind, payload } if
        kind == "launcher.favorite.toggle" && payload == "alpha.desktop"));

    assert!(update_user_launcher_state_at_path(
        &mut snapshot,
        &pref,
        "launcher.favorite.toggle",
        "alpha.desktop",
        &fixture.0
    )
    .unwrap());
    record_user_application_activation_at_path(&mut snapshot, &pref, "zeta.desktop", &fixture.0)
        .unwrap();
    record_user_application_activation_at_path(&mut snapshot, &pref, "zeta.desktop", &fixture.0)
        .unwrap();
    let rebuilt = build_launcher_menu_for(&snapshot, OverviewMode::All);
    let ids = rebuilt
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    let favorite = ids
        .iter()
        .position(|id| *id == "launcher.favorites")
        .unwrap();
    let recent = ids.iter().position(|id| *id == "launcher.recent").unwrap();
    let applications = ids.iter().position(|id| *id == "launcher.apps").unwrap();
    assert!(favorite < recent && recent < applications);
    assert_eq!(
        ids.iter()
            .filter(|id| **id == "overview.app.zeta.desktop")
            .count(),
        1
    );
    assert_eq!(
        ids.iter()
            .filter(|id| **id == "overview.app.alpha.desktop")
            .count(),
        1
    );
    assert_eq!(snapshot.recent_application_ids, vec!["zeta.desktop"]);
    let persisted = FavoritesStore::load(&fixture.0).unwrap();
    assert_eq!(persisted.recent(), &["zeta.desktop".to_owned()]);
    assert_eq!(persisted.favorites(), &["alpha.desktop".to_owned()]);

    let mut shell =
        ShellState::new(build_launcher_menu_for(&snapshot, OverviewMode::All), false).unwrap();
    shell
        .set_overview_mode(OverviewMode::Apps, &snapshot)
        .unwrap();
    shell.state.selected_id = Some("overview.app.zeta.desktop".into());
    assert!(update_user_launcher_state_at_path(
        &mut snapshot,
        &pref,
        "launcher.favorite.toggle",
        "zeta.desktop",
        &fixture.0
    )
    .unwrap());
    shell
        .refresh_family(MenuFamily::Launcher, &snapshot)
        .unwrap();
    assert_eq!(
        shell.state.selected_id.as_deref(),
        Some("overview.app.zeta.desktop")
    );

    assert!(update_user_launcher_state_at_path(
        &mut snapshot,
        &pref,
        "launcher.recent.clear",
        "",
        &fixture.0
    )
    .unwrap());
    let menu = build_launcher_menu_for(&snapshot, OverviewMode::All);
    assert!(!menu.items.iter().any(|item| item.id == "launcher.recent"));
    assert!(menu.validate().is_ok());
}

#[test]
fn stale_ids_and_invalid_actions_do_not_change_files_or_projection() {
    let fixture = FixtureState::new();
    let pref = LauncherPreferences::default();
    let mut snapshot = snapshot();
    apply_launcher_state(&mut snapshot, &FavoritesStore::default());
    assert!(!update_user_launcher_state_at_path(
        &mut snapshot,
        &pref,
        "nonsense.action",
        "alpha.desktop",
        &fixture.0
    )
    .unwrap());
    assert!(!fixture.0.exists());
    assert!(update_user_launcher_state_at_path(
        &mut snapshot,
        &pref,
        "launcher.favorite.toggle",
        "missing.desktop",
        &fixture.0
    )
    .is_err());
    assert!(!fixture.0.exists());
    assert!(record_user_application_activation_at_path(
        &mut snapshot,
        &pref,
        "../invalid.desktop",
        &fixture.0
    )
    .is_err());
    assert!(snapshot.recent_application_ids.is_empty());
}
