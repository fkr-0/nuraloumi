//! Favorites fixture and storage tests against the public shell module.
use nuraloumi_providers::{ApplicationProvider, FixtureCommandRunner, Provider};
use nuraloumi_shell::favorites;
use nuraloumi_shell::favorites::{
    state_path, FavoriteAction, FavoritesStore, MAX_FAVORITES, MAX_RECENT, MAX_STATE_BYTES,
};
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static TEST_SEQ: AtomicU64 = AtomicU64::new(0);
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        let sequence = TEST_SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "nuraloumi-favorites-tests-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&dir).expect("unique scratch directory");
        Self(dir)
    }
    fn config_path(&self) -> PathBuf {
        let path = state_path(&self.0);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        path
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn ids(items: &[&str]) -> Vec<String> {
    items.iter().map(|id| (*id).to_owned()).collect()
}

fn fixture_snapshot() -> nuraloumi_providers::ApplicationSnapshot {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/favorites");
    ApplicationProvider::new(FixtureCommandRunner::default(), vec![root])
        .with_timestamp(Some(10))
        .snapshot()
        .expect("desktop-entry fixture discovery")
}

#[test]
fn actual_desktop_discovery_drives_favorites_and_recent_projection() {
    let snapshot = fixture_snapshot();
    let discovered = snapshot
        .applications
        .iter()
        .map(|app| app.id.clone())
        .collect::<Vec<_>>();
    assert_eq!(
        discovered,
        ids(&["alpha.desktop", "terminal.desktop", "zeta.desktop"])
    );
    assert!(snapshot.applications.iter().all(|app| app.launchable));

    let dir = Scratch::new();
    let path = dir.config_path();
    let mut state = FavoritesStore::load_or_seed(
        &path,
        &ids(&["terminal.desktop", "missing.desktop", "terminal.desktop"]),
    )
    .unwrap();
    state
        .apply(
            FavoriteAction::RecordLaunch("alpha.desktop".into()),
            &discovered,
        )
        .unwrap();
    state
        .apply(
            FavoriteAction::RecordLaunch("zeta.desktop".into()),
            &discovered,
        )
        .unwrap();
    let visible = state.visible(&snapshot.applications, |app| app.id.as_str());
    assert_eq!(visible.favorites, ids(&["terminal.desktop"]));
    assert_eq!(visible.recent, ids(&["zeta.desktop", "alpha.desktop"]));
    assert!(visible.others.is_empty());
    assert_eq!(visible.all().len(), 3);
    // An ID keeps its selection even if it moves to the front.
    assert_eq!(visible.selection(Some("alpha.desktop"), 1), Some(2));
    assert_eq!(visible.selection(Some("absent.desktop"), 9), Some(2));

    state
        .apply_and_save(
            &path,
            FavoriteAction::Pin("alpha.desktop".into()),
            &discovered,
        )
        .unwrap();
    let reloaded = FavoritesStore::load(&path).unwrap();
    assert_eq!(
        reloaded.favorites(),
        &ids(&["terminal.desktop", "missing.desktop", "alpha.desktop"])
    );
    let visible = reloaded.visible(&snapshot.applications, |app| app.id.as_str());
    assert_eq!(
        visible.favorites,
        ids(&["terminal.desktop", "alpha.desktop"])
    );
    assert_eq!(visible.recent, ids(&["zeta.desktop"]));
}

#[test]
fn recent_is_mru_bounded_and_deduplicated() {
    let mut state = FavoritesStore::default();
    let available = (0..(MAX_RECENT + 3))
        .map(|n| format!("app-{n:02}.desktop"))
        .collect::<Vec<_>>();
    for id in available.iter().chain(available[..2].iter()) {
        state
            .apply(FavoriteAction::RecordLaunch(id.clone()), &available)
            .unwrap();
    }
    assert_eq!(state.recent().len(), MAX_RECENT);
    assert_eq!(
        &state.recent()[..2],
        &available[..2].iter().rev().cloned().collect::<Vec<_>>()
    );
    assert!(!state.recent().contains(&"app-02.desktop".to_owned()));
    state
        .apply(FavoriteAction::ForgetRecent("app-00.desktop".into()), &[])
        .unwrap();
    assert!(!state.recent().contains(&"app-00.desktop".to_owned()));
    state.apply(FavoriteAction::ClearRecent, &[]).unwrap();
    assert!(state.recent().is_empty());
}

#[test]
fn pin_move_toggle_and_missing_id_policy() {
    let known = ids(&["a.desktop", "b.desktop", "c.desktop"]);
    let mut state = FavoritesStore::default();
    for id in &known {
        state
            .apply(FavoriteAction::Pin(id.clone()), &known)
            .unwrap();
    }
    state
        .apply(
            FavoriteAction::Move {
                id: "c.desktop".into(),
                index: 0,
            },
            &known,
        )
        .unwrap();
    assert_eq!(
        state.favorites(),
        &ids(&["c.desktop", "a.desktop", "b.desktop"])
    );
    state
        .apply(FavoriteAction::Toggle("a.desktop".into()), &known)
        .unwrap();
    assert_eq!(state.favorites(), &ids(&["c.desktop", "b.desktop"]));
    state
        .apply(FavoriteAction::Toggle("a.desktop".into()), &known)
        .unwrap();
    assert_eq!(
        state.favorites(),
        &ids(&["c.desktop", "b.desktop", "a.desktop"])
    );

    let before = state.clone();
    assert_eq!(
        state
            .apply(FavoriteAction::Pin("missing.desktop".into()), &known)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(
        state
            .apply(
                FavoriteAction::RecordLaunch("../unsafe.desktop".into()),
                &known
            )
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(state, before);
    // Deleted desktop entries may still be unpinned without rediscovery.
    state
        .apply(FavoriteAction::Unpin("c.desktop".into()), &[])
        .unwrap();
    assert_eq!(state.favorites(), &ids(&["b.desktop", "a.desktop"]));
}

#[test]
fn favorite_limit_is_enforced_without_mutation() {
    let known = (0..=MAX_FAVORITES)
        .map(|n| format!("app-{n}.desktop"))
        .collect::<Vec<_>>();
    let mut state = FavoritesStore::default();
    for id in &known[..MAX_FAVORITES] {
        state
            .apply(FavoriteAction::Pin(id.clone()), &known)
            .unwrap();
    }
    let snapshot = state.clone();
    assert_eq!(
        state
            .apply(FavoriteAction::Pin(known[MAX_FAVORITES].clone()), &known)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(state, snapshot);
}

#[test]
fn failure_to_persist_does_not_mutate_live_state_or_leave_temporary_files() {
    let dir = Scratch::new();
    let path = dir.config_path();
    let known = ids(&["a.desktop", "b.desktop"]);
    let mut state = FavoritesStore::default();
    state
        .apply_and_save(&path, FavoriteAction::Pin("a.desktop".into()), &known)
        .unwrap();
    let saved = fs::read(&path).unwrap();

    // A directory in place of the state file cannot be atomically replaced.
    let invalid = dir.0.join("some-dir");
    fs::create_dir(&invalid).unwrap();
    let previous = state.clone();
    assert_eq!(
        state
            .apply_and_save(&invalid, FavoriteAction::Pin("b.desktop".into()), &known)
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
    assert_eq!(state, previous);
    assert_eq!(fs::read(&path).unwrap(), saved);
    assert!(fs::read_dir(path.parent().unwrap())
        .unwrap()
        .all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with(".tmp")));

    let missing_parent = dir.0.join("absent").join("launcher-state.json");
    assert_eq!(
        state
            .apply_and_save(
                &missing_parent,
                FavoriteAction::Pin("b.desktop".into()),
                &known
            )
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    assert_eq!(state, previous);
    assert!(!missing_parent.exists());

    #[cfg(unix)]
    {
        use std::os::unix::fs::{symlink, PermissionsExt};
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let symlink_path = dir.0.join("linked.json");
        symlink(&path, &symlink_path).unwrap();
        assert_eq!(
            state.save(&symlink_path).unwrap_err().kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(
            FavoritesStore::load(&symlink_path).unwrap_err().kind(),
            ErrorKind::InvalidData
        );
        assert_eq!(fs::read(&path).unwrap(), saved);
    }
}

#[test]
fn corrupt_or_unsupported_state_is_not_silently_reset() {
    let dir = Scratch::new();
    let path = dir.config_path();
    for text in [
        "not json".to_owned(),
        r#"{"version":2,"favorites":[],"recent":[]}"#.to_owned(),
        r#"{"version":1,"favorites":["../bad.desktop"],"recent":[]}"#.to_owned(),
        "x".repeat(MAX_STATE_BYTES as usize + 1),
    ] {
        fs::write(&path, text).unwrap();
        assert_eq!(
            FavoritesStore::load_or_seed(&path, &ids(&["seed.desktop"]))
                .unwrap_err()
                .kind(),
            ErrorKind::InvalidData
        );
    }
    fs::write(&path, r#"{"version":1,"favorites":["a.desktop","a.desktop"],"recent":["a.desktop","b.desktop","b.desktop"]}"#).unwrap();
    let state = FavoritesStore::load(&path).unwrap();
    assert_eq!(state.favorites(), &ids(&["a.desktop"]));
    assert_eq!(state.recent(), &ids(&["a.desktop", "b.desktop"]));
}

#[test]
fn empty_discovery_and_bad_selection_degrade_safely() {
    let mut state = FavoritesStore::default();
    state
        .apply(
            FavoriteAction::Pin("a.desktop".into()),
            &ids(&["a.desktop"]),
        )
        .unwrap();
    let visible = state.visible(&[] as &[String], String::as_str);
    assert_eq!(visible.selection(Some("a.desktop"), 0), None);
    assert!(visible.all().is_empty());
}

#[test]
fn serialized_updates_reconcile_two_independent_process_like_writers() {
    use std::sync::{Arc, Barrier};
    let dir = Scratch::new();
    let path = dir.config_path();
    let known = ids(&["alpha.desktop", "terminal.desktop", "zeta.desktop"]);
    let gate = Arc::new(Barrier::new(2));
    let workers = ["alpha.desktop", "terminal.desktop"]
        .into_iter()
        .map(|id| {
            let gate = gate.clone();
            let path = path.clone();
            let known = known.clone();
            std::thread::spawn(move || {
                gate.wait();
                favorites::update_serialized(
                    &path,
                    &[],
                    FavoriteAction::Pin(id.to_owned()),
                    &known,
                )
                .unwrap();
            })
        })
        .collect::<Vec<_>>();
    for worker in workers {
        worker.join().expect("writer thread");
    }
    let final_state = FavoritesStore::load(&path).unwrap();
    assert_eq!(final_state.favorites().len(), 2);
    assert!(final_state
        .favorites()
        .contains(&"alpha.desktop".to_owned()));
    assert!(final_state
        .favorites()
        .contains(&"terminal.desktop".to_owned()));
}

#[test]
fn serialized_update_fails_closed_when_lock_is_busy_or_symlinked() {
    let dir = Scratch::new();
    let path = dir.config_path();
    let available = ids(&["alpha.desktop"]);
    let lock_path = path.parent().unwrap().join(".launcher-state.lock");
    let lock_file = fs::File::create(&lock_path).unwrap();
    lock_file.lock().unwrap();
    let error = favorites::update_serialized(
        &path,
        &[],
        FavoriteAction::Pin("alpha.desktop".into()),
        &available,
    )
    .unwrap_err();
    assert_eq!(error.kind(), ErrorKind::WouldBlock);
    assert!(!path.exists());
    lock_file.unlock().unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        fs::remove_file(&lock_path).unwrap();
        symlink(&path, &lock_path).unwrap();
        assert_eq!(
            favorites::update_serialized(
                &path,
                &[],
                FavoriteAction::Pin("alpha.desktop".into()),
                &available
            )
            .unwrap_err()
            .kind(),
            ErrorKind::InvalidInput
        );
        assert!(!path.exists());
    }
}

#[test]
fn serialized_state_refuses_symlinked_directory() {
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        let dir = Scratch::new();
        let elsewhere = dir.0.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        let link = dir.0.join("nuraloumi");
        symlink(&elsewhere, &link).unwrap();
        let path = link.join("launcher-state.json");
        assert!(favorites::ensure_state_directory(&path).is_err());
        assert!(!elsewhere.join("launcher-state.json").exists());
    }
}
