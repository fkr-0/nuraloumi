#[path = "../src/file_manager_window.rs"]
mod model;
use model::*;
use std::{fs, path::PathBuf};

fn fixture() -> PathBuf {
    let root = std::env::temp_dir().join(format!(
        "nuraloumi-file-window-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir_all(&root).unwrap();
    root
}

#[test]
fn sorts_columns_and_keeps_selection_by_name() {
    let root = fixture();
    fs::write(root.join("z-small"), b"a").unwrap();
    fs::write(root.join("a-large"), b"longer").unwrap();
    fs::create_dir(root.join("folder")).unwrap();
    let mut model = Details::new(root.clone());
    model.refresh().unwrap();
    model.select("z-small").unwrap();
    model.toggle_sort(SortColumn::Size);
    assert_eq!(model.rows[0].name, "z-small");
    assert_eq!(model.selected.as_deref(), Some("z-small"));
    model.toggle_sort(SortColumn::Size);
    assert_eq!(model.rows[0].name, "folder");
    assert_eq!(
        model.selected_action().unwrap(),
        WindowAction::OpenFile(root.join("z-small"))
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn refuses_traversal_and_symlink_activation() {
    let root = fixture();
    let mut editor = LocationEditor::new();
    editor.begin(&root);
    editor.set("/tmp/../etc").unwrap();
    assert!(editor.accept().is_err());
    assert!(!safe_leaf("../x"));
    assert!(!safe_leaf("a/b"));
    #[cfg(unix)]
    std::os::unix::fs::symlink("/etc", root.join("outside")).unwrap();
    let mut model = Details::new(root.clone());
    model.refresh().unwrap();
    model.select("outside").unwrap();
    assert!(model.selected_action().is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn bounded_large_directory_and_completion() {
    let root = fixture();
    for n in 0..MAX_ROWS + 4 {
        fs::write(root.join(format!("item-{n:04}")), b"x").unwrap();
    }
    fs::create_dir(root.join("my-dir")).unwrap();
    let mut model = Details::new(root.clone());
    model.refresh().unwrap();
    assert_eq!(model.rows.len(), MAX_ROWS);
    assert!(model.has_more);
    let mut editor = LocationEditor::new();
    editor.begin(&root);
    editor.set(&format!("{}/my", root.display())).unwrap();
    assert!(editor.complete().iter().any(|p| p.ends_with("my-dir")));
    assert!(editor.set(&"x".repeat(MAX_LOCATION_BYTES + 1)).is_err());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn breadcrumbs_and_parent_are_root_safe() {
    assert_eq!(
        parent_location(std::path::Path::new("/")),
        PathBuf::from("/")
    );
    assert_eq!(breadcrumbs(std::path::Path::new("/tmp/a")).len(), 3);
    assert_eq!(MIN_TOUCH_PX, 48);
    let mut details = Details::new(PathBuf::from("/"));
    details.toggle_sort(SortColumn::Kind);
    details.toggle_sort(SortColumn::Modified);
    let places = builtin_places(std::path::Path::new("/tmp"));
    assert!(places.iter().any(|p| p.label == "Filesystem"));
    assert_eq!(
        sidebar_action(std::path::Path::new("/tmp"), SidebarAction::Play),
        WindowAction::PlayDirectory(PathBuf::from("/tmp"))
    );
    assert_eq!(
        sidebar_action(std::path::Path::new("/tmp"), SidebarAction::View),
        WindowAction::ViewDirectory(PathBuf::from("/tmp"))
    );
    assert_eq!(
        sidebar_action(std::path::Path::new("/tmp"), SidebarAction::Favorite),
        WindowAction::AddFavorite(PathBuf::from("/tmp"))
    );
}
