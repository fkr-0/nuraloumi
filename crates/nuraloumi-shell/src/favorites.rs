//! Bounded persistent launcher favorites and recent *application* IDs.
//!
//! XDG desktop discovery is authoritative. This module never resolves Exec,
//! launches commands, or stores arbitrary file paths. Callers execute the
//! existing "app.launch" action and record a recent ID only after success.

use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_FAVORITES: usize = 32;
pub const MAX_RECENT: usize = 32;
pub const MAX_STATE_BYTES: u64 = 8 * 1024;
const VERSION: u8 = 1;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FavoritesStore {
    favorites: Vec<String>,
    recent: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FavoriteAction {
    Pin(String),
    Unpin(String),
    Toggle(String),
    Move { id: String, index: usize },
    MoveEarlier(String),
    RecordLaunch(String),
    ForgetRecent(String),
    ClearRecent,
}

/// Sections contain desktop IDs, never commands. Each ID appears at most once.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct VisibleApplications {
    pub favorites: Vec<String>,
    pub recent: Vec<String>,
    pub others: Vec<String>,
}

impl VisibleApplications {
    pub fn all(&self) -> Vec<String> {
        self.favorites
            .iter()
            .chain(&self.recent)
            .chain(&self.others)
            .cloned()
            .collect()
    }

    /// Keep focus on the same stable application ID across reordering;
    /// if removed, keep the nearest remaining position instead.
    pub fn selection(&self, previous_id: Option<&str>, previous_index: usize) -> Option<usize> {
        let all = self.all();
        if all.is_empty() {
            return None;
        }
        Some(
            previous_id
                .and_then(|id| all.iter().position(|item| item == id))
                .unwrap_or(previous_index.min(all.len() - 1)),
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct DiskState {
    version: u8,
    favorites: Vec<String>,
    recent: Vec<String>,
}

fn bad_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn bad_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 512
        && id.ends_with(".desktop")
        && !id.contains(['\0', '\n', '\r', '/'])
}

fn dedup(ids: Vec<String>, max: usize) -> io::Result<Vec<String>> {
    if ids.len() > max {
        return Err(bad_data(
            "launcher state collection exceeds configured bound",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for id in ids {
        if !valid_id(&id) {
            return Err(bad_data(format!("invalid persisted desktop ID {id:?}")));
        }
        if seen.insert(id.clone()) {
            out.push(id);
        }
    }
    Ok(out)
}

/// Caller-supplied base directory; no global environment modification needed.
pub fn state_path(config_home: &Path) -> PathBuf {
    config_home.join("nuraloumi/launcher-state.json")
}

/// Resolve user state without requiring or modifying a Wayland session.
pub fn user_state_path() -> io::Result<PathBuf> {
    let root = match std::env::var_os("XDG_CONFIG_HOME").filter(|value| !value.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => {
            let home = std::env::var_os("HOME")
                .ok_or_else(|| bad_input("HOME or XDG_CONFIG_HOME is required"))?;
            PathBuf::from(home).join(".config")
        }
    };
    if !root.is_absolute() {
        return Err(bad_input("XDG configuration directory must be absolute"));
    }
    Ok(state_path(&root))
}

/// Ensure that the leaf state directory is real (not a symlink).
/// New directories have private permissions; an existing XDG root is respected.
pub fn ensure_state_directory(path: &Path) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| bad_input("state path has no parent"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true).mode(0o700);
        builder.create(parent)?;
    }
    #[cfg(not(unix))]
    fs::create_dir_all(parent)?;
    if !fs::symlink_metadata(parent)?.file_type().is_dir() {
        return Err(bad_input("launcher state directory may not be a symlink"));
    }
    Ok(())
}

/// One transaction with an inter-process file lock. Load inside the lock so
/// multiple menu/panel processes do not overwrite each other's recent IDs.
/// The bounded lock is taken only on the shell action path, not during paint.
pub fn update_serialized(
    path: &Path,
    initial_pins: &[String],
    action: FavoriteAction,
    discovered_ids: &[String],
) -> io::Result<FavoritesStore> {
    ensure_state_directory(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| bad_input("state path has no parent"))?;
    let lock_path = parent.join(".launcher-state.lock");
    match fs::symlink_metadata(&lock_path) {
        Ok(meta) if !meta.file_type().is_file() => {
            return Err(bad_input("launcher lock must be a regular file"))
        }
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(&lock_path)?;
    let mut locked = false;
    for _ in 0..20 {
        match lock.try_lock() {
            Ok(()) => {
                locked = true;
                break;
            }
            Err(fs::TryLockError::WouldBlock) => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            Err(fs::TryLockError::Error(error)) => return Err(error),
        }
    }
    if !locked {
        return Err(io::Error::new(
            io::ErrorKind::WouldBlock,
            "launcher state is busy; retry later",
        ));
    }

    let result = (|| {
        let mut latest = FavoritesStore::load_or_seed(path, initial_pins)?;
        latest.apply(action, discovered_ids)?;
        latest.save(path)?;
        Ok(latest)
    })();
    // The lock is also released by Drop on errors.
    let _ = lock.unlock();
    result
}

impl FavoritesStore {
    pub fn favorites(&self) -> &[String] {
        &self.favorites
    }

    pub fn recent(&self) -> &[String] {
        &self.recent
    }

    /// Use static [launcher].pinned only when no state file exists yet.
    /// Missing desktop entries stay persisted and reappear after rediscovery.
    pub fn load_or_seed(path: &Path, initial_pins: &[String]) -> io::Result<Self> {
        match Self::load(path) {
            Ok(store) => Ok(store),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let favorites = dedup(initial_pins.to_vec(), MAX_FAVORITES)
                    .map_err(|error| bad_input(error.to_string()))?;
                Ok(Self {
                    favorites,
                    recent: Vec::new(),
                })
            }
            Err(error) => Err(error),
        }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let metadata = fs::symlink_metadata(path)?;
        if !metadata.file_type().is_file() || metadata.len() > MAX_STATE_BYTES {
            return Err(bad_data("launcher state is not a bounded regular file"));
        }
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_STATE_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(bad_data("launcher state exceeds size limit"));
        }
        let disk: DiskState =
            serde_json::from_slice(&bytes).map_err(|error| bad_data(error.to_string()))?;
        if disk.version != VERSION {
            return Err(bad_data(format!(
                "unsupported launcher state version {}",
                disk.version
            )));
        }
        Ok(Self {
            favorites: dedup(disk.favorites, MAX_FAVORITES)?,
            recent: dedup(disk.recent, MAX_RECENT)?,
        })
    }

    /// Project existing discovered apps in stable store order, then in the
    /// discovery provider's deterministic order. Absent IDs are simply omitted.
    pub fn visible<T>(
        &self,
        applications: &[T],
        id_of: impl Fn(&T) -> &str,
    ) -> VisibleApplications {
        let mut available = BTreeSet::new();
        let mut discovery_order = Vec::new();
        for app in applications {
            let id = id_of(app);
            if valid_id(id) && available.insert(id.to_owned()) {
                discovery_order.push(id.to_owned());
            }
        }
        let favorites = self
            .favorites
            .iter()
            .filter(|id| available.contains(*id))
            .cloned()
            .collect::<Vec<_>>();
        let favored = favorites
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let recent = self
            .recent
            .iter()
            .filter(|id| available.contains(*id) && !favored.contains(id.as_str()))
            .cloned()
            .collect::<Vec<_>>();
        let featured = favorites
            .iter()
            .chain(&recent)
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let others = discovery_order
            .into_iter()
            .filter(|id| !featured.contains(id.as_str()))
            .collect();
        VisibleApplications {
            favorites,
            recent,
            others,
        }
    }

    /// Explicit mutations require known discovered desktop IDs for adding
    /// pins or launches. Removal is allowed even if the entry disappeared.
    pub fn apply(&mut self, action: FavoriteAction, discovered_ids: &[String]) -> io::Result<()> {
        use FavoriteAction::*;
        let id = match &action {
            Pin(id)
            | Unpin(id)
            | Toggle(id)
            | Move { id, .. }
            | MoveEarlier(id)
            | RecordLaunch(id)
            | ForgetRecent(id) => Some(id.as_str()),
            ClearRecent => None,
        };
        if let Some(id) = id {
            if !valid_id(id) {
                return Err(bad_input("invalid launcher desktop ID"));
            }
            if matches!(action, Pin(_) | RecordLaunch(_))
                && !discovered_ids.iter().any(|available| available == id)
            {
                return Err(bad_input("desktop ID is not currently discovered"));
            }
        }
        match action {
            Pin(id) => {
                if !self.favorites.contains(&id) {
                    if self.favorites.len() == MAX_FAVORITES {
                        return Err(bad_input("favorite limit reached"));
                    }
                    self.favorites.push(id);
                }
            }
            Unpin(id) => self.favorites.retain(|old| old != &id),
            Toggle(id) => {
                if self.favorites.contains(&id) {
                    self.favorites.retain(|old| old != &id);
                } else {
                    return self.apply(Pin(id), discovered_ids);
                }
            }
            Move { id, index } => {
                if let Some(old) = self.favorites.iter().position(|item| item == &id) {
                    self.favorites.remove(old);
                    self.favorites.insert(index.min(self.favorites.len()), id);
                } else {
                    return Err(bad_input("cannot move an unpinned application"));
                }
            }
            MoveEarlier(id) => {
                if let Some(index) = self.favorites.iter().position(|item| item == &id) {
                    if index > 0 {
                        self.favorites.swap(index - 1, index);
                    }
                } else {
                    return Err(bad_input("cannot move an unpinned application"));
                }
            }
            RecordLaunch(id) => {
                self.recent.retain(|old| old != &id);
                self.recent.insert(0, id);
                self.recent.truncate(MAX_RECENT);
            }
            ForgetRecent(id) => self.recent.retain(|old| old != &id),
            ClearRecent => self.recent.clear(),
        }
        Ok(())
    }

    /// Commit in-memory state only if the entire on-disk replacement succeeds.
    pub fn apply_and_save(
        &mut self,
        path: &Path,
        action: FavoriteAction,
        discovered_ids: &[String],
    ) -> io::Result<()> {
        let mut next = self.clone();
        next.apply(action, discovered_ids)?;
        next.save(path)?;
        *self = next;
        Ok(())
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        // Validate all fields before opening or replacing any file.
        let disk = DiskState {
            version: VERSION,
            favorites: dedup(self.favorites.clone(), MAX_FAVORITES)
                .map_err(|error| bad_input(error.to_string()))?,
            recent: dedup(self.recent.clone(), MAX_RECENT)
                .map_err(|error| bad_input(error.to_string()))?,
        };
        let bytes = serde_json::to_vec(&disk).map_err(io::Error::other)?;
        if bytes.len() as u64 > MAX_STATE_BYTES {
            return Err(bad_input("launcher state exceeds size bound"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| bad_input("state path has no parent"))?;
        // A caller may create XDG config directories, but this module does not
        // traverse arbitrary symlinked state directories.
        let parent_meta = fs::symlink_metadata(parent)?;
        if !parent_meta.file_type().is_dir() {
            return Err(bad_input("state directory is not a real directory"));
        }
        match fs::symlink_metadata(path) {
            Ok(meta) if !meta.file_type().is_file() => {
                return Err(bad_input("refusing to replace non-regular launcher state"))
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
            _ => {}
        }

        // create_new protects against temporary-file clobbering. Rename within
        // the same directory is atomic, and sync_all makes contents durable.
        for _ in 0..16 {
            let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
            let name = format!(".launcher-state-{}-{sequence}.tmp", std::process::id());
            let temporary = parent.join(name);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let file = match options.open(&temporary) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            };
            let result = (|| {
                let mut file = file;
                file.write_all(&bytes)?;
                file.sync_all()?;
                fs::rename(&temporary, path)?;
                File::open(parent)?.sync_all()
            })();
            if result.is_err() {
                let _ = fs::remove_file(&temporary);
            }
            return result;
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "exhausted temporary launcher state names",
        ))
    }
}
