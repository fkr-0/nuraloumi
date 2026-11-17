//! File manager window state, independent of the compositor and rendering.
//! The real xdg_toplevel integration is deliberately a separate backend seam.
use std::cmp::Ordering;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::SystemTime;

pub const MAX_ROWS: usize = 512;
pub const MAX_LOCATION_BYTES: usize = 4096;
pub const MAX_COMPLETIONS: usize = 16;
pub const MIN_TOUCH_PX: u32 = 48;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortColumn {
    Name,
    Kind,
    Size,
    Modified,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortSpec {
    pub column: SortColumn,
    pub descending: bool,
}

impl Default for SortSpec {
    fn default() -> Self {
        Self {
            column: SortColumn::Name,
            descending: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowKind {
    Directory,
    RegularFile,
    Symlink,
    Other,
}

#[derive(Clone, Debug)]
pub struct DetailRow {
    pub name: String,
    pub kind: RowKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

#[derive(Clone, Debug)]
pub struct Details {
    pub directory: PathBuf,
    pub rows: Vec<DetailRow>,
    pub selected: Option<String>,
    pub scroll: usize,
    pub sort: SortSpec,
    pub has_more: bool,
    pub message: String,
}

impl Details {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            rows: Vec::new(),
            selected: None,
            scroll: 0,
            sort: SortSpec::default(),
            has_more: false,
            message: String::new(),
        }
    }

    /// Retains at most MAX_ROWS; filesystem calls must run on a worker, not in paint().
    /// This deliberately fails closed for operations targeting unlisted entries.
    pub fn refresh(&mut self) -> Result<(), String> {
        let root =
            fs::canonicalize(&self.directory).map_err(|e| format!("resolve directory: {e}"))?;
        if !root.is_dir() {
            return Err("location is not a directory".into());
        }
        let iter = fs::read_dir(&root).map_err(|e| format!("read directory: {e}"))?;
        let mut rows = Vec::with_capacity(MAX_ROWS);
        let mut more = false;
        for item in iter {
            if rows.len() == MAX_ROWS {
                more = true;
                break;
            }
            let item = item.map_err(|e| format!("read directory entry: {e}"))?;
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !safe_leaf(&name) {
                continue;
            }
            let metadata = fs::symlink_metadata(item.path())
                .map_err(|e| format!("stat directory entry: {e}"))?;
            let kind = if metadata.file_type().is_symlink() {
                RowKind::Symlink
            } else if metadata.is_dir() {
                RowKind::Directory
            } else if metadata.is_file() {
                RowKind::RegularFile
            } else {
                RowKind::Other
            };
            rows.push(DetailRow {
                name,
                kind,
                size: metadata.len(),
                modified: metadata.modified().ok(),
            });
        }
        self.directory = root;
        self.has_more = more;
        self.message = if more {
            format!("Showing first {MAX_ROWS} items; additional entries require pagination")
        } else {
            String::new()
        };
        self.rows = rows;
        self.sort_rows();
        if !self
            .selected
            .as_ref()
            .is_some_and(|name| self.rows.iter().any(|r| &r.name == name))
        {
            self.selected = None;
        }
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(1));
        Ok(())
    }

    pub fn toggle_sort(&mut self, column: SortColumn) {
        self.sort = SortSpec {
            column,
            descending: self.sort.column == column && !self.sort.descending,
        };
        self.sort_rows();
    }

    fn sort_rows(&mut self) {
        let spec = self.sort;
        self.rows.sort_by(|a, b| {
            let cmp = match spec.column {
                SortColumn::Name => fold_name(&a.name).cmp(&fold_name(&b.name)),
                SortColumn::Kind => kind_rank(&a.kind).cmp(&kind_rank(&b.kind)),
                SortColumn::Size => a.size.cmp(&b.size),
                SortColumn::Modified => a.modified.cmp(&b.modified),
            };
            let cmp = if spec.descending { cmp.reverse() } else { cmp };
            if cmp == Ordering::Equal {
                a.name.cmp(&b.name)
            } else {
                cmp
            }
        });
    }

    pub fn select(&mut self, name: &str) -> Result<(), String> {
        if !safe_leaf(name) || !self.rows.iter().any(|row| row.name == name) {
            return Err("entry not present in current snapshot".into());
        }
        self.selected = Some(name.to_owned());
        Ok(())
    }

    /// Returns only typed intent; execution must validate/reopen on a worker.
    pub fn selected_action(&self) -> Result<WindowAction, String> {
        let name = self.selected.as_ref().ok_or("nothing selected")?;
        let row = self
            .rows
            .iter()
            .find(|row| &row.name == name)
            .ok_or("selection is stale")?;
        let path = self.directory.join(name);
        match row.kind {
            RowKind::Directory => Ok(WindowAction::Navigate(path)),
            RowKind::RegularFile => Ok(WindowAction::OpenFile(path)),
            RowKind::Symlink | RowKind::Other => {
                Err("special files and symlinks are not opened".into())
            }
        }
    }
}

fn kind_rank(kind: &RowKind) -> u8 {
    match kind {
        RowKind::Directory => 0,
        RowKind::RegularFile => 1,
        RowKind::Symlink => 2,
        RowKind::Other => 3,
    }
}

fn fold_name(value: &str) -> String {
    value.to_lowercase()
}

pub fn safe_leaf(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 255
        && !name.contains(['/', '\\', '\0', '\r', '\n'])
        && !name.chars().any(char::is_control)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WindowAction {
    Navigate(PathBuf),
    OpenFile(PathBuf),
    AddFavorite(PathBuf),
    PlayDirectory(PathBuf),
    ViewDirectory(PathBuf),
}

#[derive(Clone, Debug)]
pub struct LocationEditor {
    pub active: bool,
    pub text: String,
}

impl LocationEditor {
    pub fn new() -> Self {
        Self {
            active: false,
            text: String::new(),
        }
    }
    pub fn begin(&mut self, directory: &Path) {
        self.active = true;
        self.text = directory.to_string_lossy().into_owned();
    }
    pub fn set(&mut self, text: &str) -> Result<(), String> {
        if text.len() > MAX_LOCATION_BYTES || text.chars().any(char::is_control) {
            return Err("invalid or oversized location".into());
        }
        self.text = text.into();
        Ok(())
    }
    pub fn accept(&mut self) -> Result<PathBuf, String> {
        let path = Path::new(&self.text);
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err("use an absolute location without '..'".into());
        }
        let canonical = fs::canonicalize(path).map_err(|e| format!("resolve location: {e}"))?;
        if !canonical.is_dir() {
            return Err("location is not a directory".into());
        }
        self.active = false;
        Ok(canonical)
    }
    pub fn complete(&self) -> Vec<String> {
        if !self.active || self.text.len() > MAX_LOCATION_BYTES || !self.text.starts_with('/') {
            return Vec::new();
        }
        let path = Path::new(&self.text);
        let (parent, prefix) = if self.text.ends_with('/') {
            (path, "")
        } else {
            (
                path.parent().unwrap_or(Path::new("/")),
                path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            )
        };
        if parent
            .components()
            .any(|p| matches!(p, Component::ParentDir))
        {
            return Vec::new();
        }
        let Ok(entries) = fs::read_dir(parent) else {
            return Vec::new();
        };
        let mut matches = Vec::new();
        // A completion operation examines at most 4096 entries.
        for item in entries.take(4096).flatten() {
            let Some(name) = item.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if !safe_leaf(&name) || !name.starts_with(prefix) {
                continue;
            }
            if !item.file_type().is_ok_and(|kind| kind.is_dir()) {
                continue;
            }
            matches.push(parent.join(name).to_string_lossy().into_owned());
            if matches.len() == MAX_COMPLETIONS {
                break;
            }
        }
        matches.sort();
        matches
    }
}

impl Default for LocationEditor {
    fn default() -> Self {
        Self::new()
    }
}

pub fn breadcrumbs(path: &Path) -> Vec<(String, PathBuf)> {
    let mut result = vec![("/".into(), PathBuf::from("/"))];
    let mut cursor = PathBuf::from("/");
    for part in path.components() {
        if let Component::Normal(name) = part {
            cursor.push(name);
            result.push((name.to_string_lossy().into_owned(), cursor.clone()));
        }
    }
    result
}

pub fn parent_location(path: &Path) -> PathBuf {
    path.parent().unwrap_or(Path::new("/")).to_path_buf()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Place {
    pub label: String,
    pub path: PathBuf,
}

/// Places are derived locally; never parse mounts by executing a command.
pub fn builtin_places(home: &Path) -> Vec<Place> {
    let mut places = vec![Place {
        label: "Home".into(),
        path: home.to_path_buf(),
    }];
    let downloads = home.join("Downloads");
    if downloads.is_dir() {
        places.push(Place {
            label: "Downloads".into(),
            path: downloads,
        });
    }
    places.push(Place {
        label: "Filesystem".into(),
        path: PathBuf::from("/"),
    });
    places.push(Place {
        label: "Temporary".into(),
        path: PathBuf::from("/tmp"),
    });
    places
}

pub fn sidebar_action(directory: &Path, action: SidebarAction) -> WindowAction {
    match action {
        SidebarAction::Favorite => WindowAction::AddFavorite(directory.to_path_buf()),
        SidebarAction::Play => WindowAction::PlayDirectory(directory.to_path_buf()),
        SidebarAction::View => WindowAction::ViewDirectory(directory.to_path_buf()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidebarAction {
    Favorite,
    Play,
    View,
}
