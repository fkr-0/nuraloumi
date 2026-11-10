//! Rooted, bounded quick-browser operations. No command strings or renderer effects.
use std::collections::BTreeMap;
use std::fs;
use std::io;
#[cfg(target_os = "linux")]
use std::os::fd::{AsRawFd, FromRawFd};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

// Retained entry count is bounded; every entry remains reachable by advancing
// to the next batch. Scanning is exhaustive and must happen off the UI thread.
pub const MAX_VISIBLE_ENTRIES: usize = 512;
pub const MAX_FILENAME_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub directory: bool,
    pub len: u64,
    #[cfg(unix)]
    pub device: u64,
    #[cfg(unix)]
    pub inode: u64,
}

// Linux has no unlinkat() variant that atomically compares a pathname with an
// opened fd/inode. After a verified parent lookup, a concurrent rename may
// still replace the leaf before unlinkat or renameat2. Move the leaf into an
// unguessable, owner-private staging directory first, verify the *moved*
// object, and only then perform the final destructive operation. An unexpected
// object is never unlinked; we attempt an atomic no-clobber rollback instead.
//
// This protects against ordinary concurrent changes and other users. It is
// NOT a guarantee against a hostile process with the same UID and write
// access to the private staging directory; such a process can race Linux's
// pathname-based unlinkat as well. The UI should not claim otherwise.
#[cfg(target_os = "linux")]
struct MutationStage {
    dir: fs::File,
    name: String,
}

#[cfg(target_os = "linux")]
impl MutationStage {
    fn create(parent: &fs::File) -> io::Result<Self> {
        use std::ffi::CString;
        use std::io::Read;
        use std::os::raw::{c_char, c_int, c_uint};
        unsafe extern "C" {
            fn mkdirat(dirfd: c_int, pathname: *const c_char, mode: c_uint) -> c_int;
            fn openat(dirfd: c_int, pathname: *const c_char, flags: c_int) -> c_int;
        }
        const O_DIRECTORY: c_int = 0o200000;
        const O_NOFOLLOW: c_int = 0o400000;
        const O_CLOEXEC: c_int = 0o2000000;
        let mut entropy = fs::File::open("/dev/urandom")?;
        for _ in 0..4 {
            let mut random = [0_u8; 16];
            entropy.read_exact(&mut random)?;
            let name = format!(
                ".nuraloumi-stage-{}",
                random
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>()
            );
            let c_name = CString::new(name.as_str()).map_err(io::Error::other)?;
            if unsafe { mkdirat(parent.as_raw_fd(), c_name.as_ptr(), 0o700) } < 0 {
                let error = io::Error::last_os_error();
                if error.kind() == io::ErrorKind::AlreadyExists {
                    continue;
                }
                return Err(error);
            }
            let fd = unsafe {
                openat(
                    parent.as_raw_fd(),
                    c_name.as_ptr(),
                    O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                )
            };
            if fd < 0 {
                let error = io::Error::last_os_error();
                let _ = delete_at(parent, &name, true);
                return Err(error);
            }
            let dir = unsafe { fs::File::from_raw_fd(fd) };
            // A private directory is not enough if the owner changed between
            // mkdirat and openat. Fail closed if it is not owned by us and
            // mode 0700; no subsequent entry is moved into an unsafe location.
            let metadata = dir.metadata()?;
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if metadata.uid() != unsafe { geteuid() }
                || metadata.permissions().mode() & 0o777 != 0o700
            {
                let _ = delete_at(parent, &name, true);
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "staging directory is not owner-private",
                ));
            }
            return Ok(Self { dir, name });
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "stage name collisions",
        ))
    }

    fn cleanup(&self, parent: &fs::File) {
        // Only works when empty. A failed rollback deliberately retains the
        // staged entry for manual recovery instead of destroying user data.
        let _ = delete_at(parent, &self.name, true);
    }
}

#[cfg(target_os = "linux")]
unsafe extern "C" {
    fn geteuid() -> std::os::raw::c_uint;
}

#[cfg(target_os = "linux")]
fn staged_entry_metadata(stage: &fs::File) -> io::Result<fs::Metadata> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int};
    unsafe extern "C" {
        fn openat(dirfd: c_int, pathname: *const c_char, flags: c_int) -> c_int;
    }
    const O_PATH: c_int = 0o10000000;
    const O_NOFOLLOW: c_int = 0o400000;
    const O_CLOEXEC: c_int = 0o2000000;
    let name = CString::new("entry").map_err(io::Error::other)?;
    let fd = unsafe {
        openat(
            stage.as_raw_fd(),
            name.as_ptr(),
            O_PATH | O_NOFOLLOW | O_CLOEXEC,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let selected = unsafe { fs::File::from_raw_fd(fd) };
    selected.metadata()
}

#[cfg(target_os = "linux")]
fn mutate_verified_at(
    parent: &fs::File,
    pinned_entry: &fs::File,
    expected: &Entry,
    destination: Option<&str>,
) -> Result<(), String> {
    let stage = MutationStage::create(parent).map_err(|e| err("create private staging", e))?;
    if let Err(error) = rename_between_at(parent, &expected.name, &stage.dir, "entry") {
        stage.cleanup(parent);
        return Err(err("stage selected entry", error));
    }
    let actual = staged_entry_metadata(&stage.dir);
    let pinned = pinned_entry.metadata();
    let matches_expected = actual.as_ref().is_ok_and(|metadata| {
        pinned.as_ref().is_ok_and(|original| {
            // The O_PATH pin prevents source inode reuse (ABA) even if the
            // source pathname is unlinked/recreated while we are staging it.
            !metadata.file_type().is_symlink()
                && expected.directory == metadata.is_dir()
                && expected.device == metadata.dev()
                && expected.inode == metadata.ino()
                && original.dev() == metadata.dev()
                && original.ino() == metadata.ino()
        })
    });
    let result = if !matches_expected {
        Err("selected entry changed after validation; operation cancelled".into())
    } else if let Some(dest) = destination {
        rename_between_at(&stage.dir, "entry", parent, dest).map_err(|e| err("finish rename", e))
    } else {
        delete_at(&stage.dir, "entry", expected.directory).map_err(|e| err("finish delete", e))
    };
    if let Err(ref failure) = result {
        if let Err(rollback) = rename_between_at(&stage.dir, "entry", parent, &expected.name) {
            // Preserve the unexpected object under a recoverable hidden path.
            // Never remove stage/entry after a failed rollback.
            return Err(format!(
                "{failure}; rollback failed: {rollback}; recovery: {}/entry",
                stage.name,
            ));
        }
    }
    stage.cleanup(parent);
    result
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct EntryKey {
    directory_rank: u8,
    folded: String,
    name: String,
}
impl Entry {
    fn order_key(&self) -> EntryKey {
        EntryKey {
            directory_rank: u8::from(!self.directory),
            folded: self.name.to_ascii_lowercase(),
            name: self.name.clone(),
        }
    }
}

#[cfg(target_os = "linux")]
fn delete_at(dir: &fs::File, name: &str, is_directory: bool) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int};
    unsafe extern "C" {
        fn unlinkat(dirfd: c_int, pathname: *const c_char, flags: c_int) -> c_int;
    }
    const AT_REMOVEDIR: c_int = 0x200;
    let name = CString::new(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in filename"))?;
    if unsafe {
        unlinkat(
            dir.as_raw_fd(),
            name.as_ptr(),
            if is_directory { AT_REMOVEDIR } else { 0 },
        )
    } == 0
    {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}
#[derive(Debug, Clone, Copy, Default)]
pub struct Capabilities {
    pub rename: bool,
    pub delete: bool,
}
pub struct Browser {
    pub root: PathBuf,
    pub current: PathBuf,
    // Keep the original root inode pinned; destructive operations must never
    // re-resolve a mutable absolute pathname as their authority.
    #[cfg(target_os = "linux")]
    root_handle: fs::File,
    pub entries: Vec<Entry>,
    pub has_more: bool,
    pub scanned: usize,
    pub message: String,
    pub capabilities: Capabilities,
    batch_cursor: Option<EntryKey>,
    batch_history: Vec<Option<EntryKey>>,
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= MAX_FILENAME_BYTES
        && !name.contains(['/', '\\', '\0', '\r', '\n'])
        && !name.chars().any(char::is_control)
}
pub fn valid_desktop_id(id: &str) -> bool {
    id.len() <= 512
        && id.ends_with(".desktop")
        && id != ".desktop"
        && !id.contains(['/', '\\', '\0', '\r', '\n'])
}
fn err(ctx: &str, e: io::Error) -> String {
    format!("{ctx}: {e}")
}
impl Browser {
    pub fn new(root: &Path, capabilities: Capabilities) -> Result<Self, String> {
        let mut browse = Self::new_unscanned(root, capabilities)?;
        browse.refresh()?;
        Ok(browse)
    }
    /// A worker-owned snapshot that can scan and navigate without locking the
    /// Wayland UI model. The pinned root descriptor is safely duplicated.
    pub fn try_clone(&self) -> Result<Self, String> {
        Ok(Self {
            root: self.root.clone(),
            current: self.current.clone(),
            #[cfg(target_os = "linux")]
            root_handle: self
                .root_handle
                .try_clone()
                .map_err(|e| err("duplicate browser root", e))?,
            entries: self.entries.clone(),
            has_more: self.has_more,
            scanned: self.scanned,
            message: self.message.clone(),
            capabilities: self.capabilities,
            batch_cursor: self.batch_cursor.clone(),
            batch_history: self.batch_history.clone(),
        })
    }
    /// Construct the browser without scanning directory contents; the live
    /// Wayland frontend schedules the first scan on the directory worker.
    pub fn new_unscanned(root: &Path, capabilities: Capabilities) -> Result<Self, String> {
        let root = fs::canonicalize(root).map_err(|e| err("resolve browser root", e))?;
        if !root.is_dir() {
            return Err("browser root is not a directory".into());
        }
        #[cfg(target_os = "linux")]
        let root_handle = {
            use std::os::unix::fs::OpenOptionsExt;
            // Avoid following a swapped final symlink between canonicalize
            // and pinning the original root directory.
            fs::OpenOptions::new()
                .read(true)
                .custom_flags(0o200000 | 0o400000 | 0o2000000)
                .open(&root)
                .map_err(|e| err("pin browser root", e))?
        };
        let browse = Self {
            current: root.clone(),
            root,
            #[cfg(target_os = "linux")]
            root_handle,
            entries: Vec::new(),
            has_more: false,
            scanned: 0,
            message: "Read-only by default; R rename / D delete / O open-with".into(),
            capabilities,
            batch_cursor: None,
            batch_history: Vec::new(),
        };
        Ok(browse)
    }
    fn ensure_current(&self) -> Result<(), String> {
        let path =
            fs::canonicalize(&self.current).map_err(|e| err("resolve current directory", e))?;
        if !path.starts_with(&self.root) || path != self.current {
            return Err("directory escaped root or became a symlink".into());
        }
        Ok(())
    }

    /// Advance through the entire directory using a bounded sort selection.
    /// Each batch retains at most MAX_VISIBLE_ENTRIES, including huge folders.
    pub fn next_batch(&mut self) -> Result<bool, String> {
        if !self.has_more {
            return Ok(false);
        }
        let next = self
            .entries
            .last()
            .ok_or("missing batch boundary")?
            .order_key();
        let previous = self.batch_cursor.replace(next);
        self.batch_history.push(previous.clone());
        if let Err(error) = self.refresh() {
            self.batch_history.pop();
            self.batch_cursor = previous;
            return Err(error);
        }
        Ok(true)
    }

    pub fn previous_batch(&mut self) -> Result<bool, String> {
        let Some(previous) = self.batch_history.pop() else {
            return Ok(false);
        };
        let old = std::mem::replace(&mut self.batch_cursor, previous.clone());
        if let Err(error) = self.refresh() {
            self.batch_history.push(previous);
            self.batch_cursor = old;
            return Err(error);
        }
        Ok(true)
    }
    pub fn batch_number(&self) -> usize {
        self.batch_history.len() + 1
    }
    pub fn has_previous_batch(&self) -> bool {
        !self.batch_history.is_empty()
    }
    fn path_for(&self, index: usize) -> Result<(PathBuf, fs::Metadata), String> {
        self.ensure_current()?;
        let name = &self
            .entries
            .get(index)
            .ok_or("entry index out of bounds")?
            .name;
        if !valid_name(name) {
            return Err("unsafe entry name".into());
        }
        let path = self.current.join(name);
        let meta = fs::symlink_metadata(&path).map_err(|e| err("inspect entry", e))?;
        if meta.file_type().is_symlink() || (!meta.is_file() && !meta.is_dir()) {
            return Err("symlinks and special files cannot be acted upon".into());
        }
        let entry = &self.entries[index];
        if entry.directory != meta.is_dir() {
            return Err("entry kind changed since scan; refresh required".into());
        }
        #[cfg(unix)]
        if entry.device != meta.dev() || entry.inode != meta.ino() {
            return Err("entry identity changed since scan; refresh required".into());
        }
        Ok((path, meta))
    }

    // Walk only below the pinned root, one component at a time. O_NOFOLLOW
    // applies to every component, not just to the selected file. Returning
    // the opened directory keeps the eventual *at() syscall independent of
    // mutable parent pathnames.
    #[cfg(target_os = "linux")]
    fn anchored_parent(&self) -> Result<fs::File, String> {
        use std::ffi::CString;
        use std::os::raw::{c_char, c_int};
        use std::os::unix::ffi::OsStrExt;
        unsafe extern "C" {
            fn openat(dirfd: c_int, pathname: *const c_char, flags: c_int) -> c_int;
        }
        const O_DIRECTORY: c_int = 0o200000;
        const O_NOFOLLOW: c_int = 0o400000;
        const O_CLOEXEC: c_int = 0o2000000;
        let relative = self
            .current
            .strip_prefix(&self.root)
            .map_err(|_| "directory outside pinned root")?;
        let mut parent = self
            .root_handle
            .try_clone()
            .map_err(|e| err("duplicate root descriptor", e))?;
        for part in relative.components() {
            let std::path::Component::Normal(name) = part else {
                return Err("invalid directory component".into());
            };
            let name = CString::new(name.as_bytes()).map_err(|_| "NUL in directory name")?;
            let fd = unsafe {
                openat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                )
            };
            if fd < 0 {
                return Err(err("open rooted directory", io::Error::last_os_error()));
            }
            parent = unsafe { fs::File::from_raw_fd(fd) };
        }
        Ok(parent)
    }

    #[cfg(target_os = "linux")]
    fn anchored_parent_for(&self, index: usize) -> Result<(fs::File, fs::File), String> {
        use std::ffi::CString;
        use std::os::raw::{c_char, c_int};
        unsafe extern "C" {
            fn openat(dirfd: c_int, pathname: *const c_char, flags: c_int) -> c_int;
        }
        const O_PATH: c_int = 0o10000000;
        const O_NOFOLLOW: c_int = 0o400000;
        const O_CLOEXEC: c_int = 0o2000000;
        let parent = self.anchored_parent()?;
        let visible_parent =
            fs::symlink_metadata(&self.current).map_err(|e| err("check current dir", e))?;
        let anchored_meta = parent
            .metadata()
            .map_err(|e| err("check anchored dir", e))?;
        if visible_parent.dev() != anchored_meta.dev()
            || visible_parent.ino() != anchored_meta.ino()
        {
            return Err("directory replaced before rooted mutation".into());
        }
        let entry = self.entries.get(index).ok_or("entry index out of bounds")?;
        let name = CString::new(entry.name.as_str()).map_err(|_| "NUL in entry name")?;
        let fd = unsafe {
            openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                O_PATH | O_NOFOLLOW | O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(err("open rooted entry", io::Error::last_os_error()));
        }
        let selected = unsafe { fs::File::from_raw_fd(fd) };
        let actual = selected
            .metadata()
            .map_err(|e| err("check rooted entry", e))?;
        if actual.file_type().is_symlink()
            || entry.directory != actual.is_dir()
            || entry.device != actual.dev()
            || entry.inode != actual.ino()
        {
            return Err("entry replaced before rooted mutation".into());
        }
        Ok((parent, selected))
    }
    pub fn refresh(&mut self) -> Result<(), String> {
        self.ensure_current()?;
        let entries = fs::read_dir(&self.current).map_err(|e| err("list directory", e))?;
        let mut selected = BTreeMap::<EntryKey, Entry>::new();
        let mut rejected = 0usize;
        let mut scanned = 0usize;
        let mut recovery_stages = 0usize;
        for e in entries {
            scanned = scanned.saturating_add(1);
            let entry = e.map_err(|e| err("read entry", e))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                rejected += 1;
                continue;
            };
            if name.starts_with(".nuraloumi-stage-") {
                recovery_stages = recovery_stages.saturating_add(1);
            }
            if name.starts_with('.') || !valid_name(&name) {
                rejected += 1;
                continue;
            }
            let meta = match fs::symlink_metadata(entry.path()) {
                Ok(meta) => meta,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {
                    rejected += 1;
                    continue;
                }
                Err(e) => return Err(err("stat entry", e)),
            };
            if meta.file_type().is_symlink() || (!meta.is_dir() && !meta.is_file()) {
                rejected += 1;
                continue;
            }
            let entry = Entry {
                name,
                directory: meta.is_dir(),
                len: meta.len(),
                #[cfg(unix)]
                device: meta.dev(),
                #[cfg(unix)]
                inode: meta.ino(),
            };
            let key = entry.order_key();
            if self
                .batch_cursor
                .as_ref()
                .is_some_and(|cursor| key <= *cursor)
            {
                continue;
            }
            selected.insert(key, entry);
            if selected.len() > MAX_VISIBLE_ENTRIES + 1 {
                selected.pop_last();
            }
        }
        let has_more = selected.len() > MAX_VISIBLE_ENTRIES;
        if has_more {
            selected.pop_last();
        }
        let result = selected.into_values().collect::<Vec<_>>();
        self.scanned = scanned;
        self.has_more = has_more;
        self.entries = result;
        self.message = if has_more {
            format!(
                "Batch {}: {} shown of {scanned} scanned; next batch available",
                self.batch_history.len() + 1,
                self.entries.len()
            )
        } else if !self.batch_history.is_empty() {
            format!(
                "Batch {}: {} shown of {scanned} scanned; previous batch available",
                self.batch_history.len() + 1,
                self.entries.len()
            )
        } else if rejected > 0 {
            format!(
                "{} visible; skipped {rejected} hidden/unsafe entries",
                self.entries.len()
            )
        } else {
            format!(
                "{} entries | Enter open | O chooser | Esc back",
                self.entries.len()
            )
        };
        if recovery_stages != 0 {
            // Interrupted staged operations retain data instead of silently
            // deleting it. Flag recovery to the user; never auto-delete a
            // private staging directory on startup.
            self.message.push_str(&format!(
                " | WARNING: {recovery_stages} interrupted mutation staging folder(s); recover .nuraloumi-stage-*/entry manually"
            ));
        }
        Ok(())
    }
    /// Validate against the last scan and current inode before passing a file
    /// to an external opener. Never follow a symlink or hand out a directory.
    pub fn regular_file(&self, index: usize) -> Result<PathBuf, String> {
        let (path, metadata) = self.path_for(index)?;
        if !metadata.is_file() {
            return Err("selected entry is not a regular file".into());
        }
        Ok(path)
    }

    pub fn enter(&mut self, index: usize) -> Result<bool, String> {
        let (path, meta) = self.path_for(index)?;
        if !meta.is_dir() {
            return Ok(false);
        }
        let next = fs::canonicalize(&path).map_err(|e| err("resolve subdirectory", e))?;
        if !next.starts_with(&self.root) {
            return Err("directory escapes configured root".into());
        }
        let previous = std::mem::replace(&mut self.current, next);
        let old_cursor = self.batch_cursor.take();
        let old_history = std::mem::take(&mut self.batch_history);
        if let Err(error) = self.refresh() {
            self.current = previous;
            self.batch_cursor = old_cursor;
            self.batch_history = old_history;
            return Err(error);
        }
        Ok(true)
    }
    pub fn parent(&mut self) -> Result<bool, String> {
        self.ensure_current()?;
        if self.current == self.root {
            return Ok(false);
        }
        let next = self.current.parent().ok_or("missing parent")?.to_path_buf();
        let previous = std::mem::replace(&mut self.current, next);
        let old_cursor = self.batch_cursor.take();
        let old_history = std::mem::take(&mut self.batch_history);
        if let Err(error) = self.refresh() {
            self.current = previous;
            self.batch_cursor = old_cursor;
            self.batch_history = old_history;
            return Err(error);
        }
        Ok(true)
    }
    pub fn rename(&mut self, index: usize, new_name: &str, confirmed: bool) -> Result<(), String> {
        if !self.capabilities.rename {
            return Err("rename capability disabled".into());
        }
        if !confirmed {
            return Err("explicit rename confirmation required".into());
        }
        if !valid_name(new_name) {
            return Err("invalid new filename".into());
        }
        let (old, _) = self.path_for(index)?;
        let dest = self.current.join(new_name);
        if dest == old {
            return Err("rename target unchanged".into());
        }
        match fs::symlink_metadata(&dest) {
            Ok(_) => return Err("rename target already exists".into()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(err("inspect rename target", e)),
        }
        #[cfg(target_os = "linux")]
        {
            let (parent, pinned) = self.anchored_parent_for(index)?;
            mutate_verified_at(&parent, &pinned, &self.entries[index], Some(new_name))?;
        }
        #[cfg(not(target_os = "linux"))]
        rename_without_replacing(&old, &dest).map_err(|e| err("rename", e))?;
        self.refresh()?;
        self.message = format!("Renamed to {new_name}");
        Ok(())
    }
    pub fn delete(
        &mut self,
        index: usize,
        expected_name: &str,
        confirmed: bool,
    ) -> Result<(), String> {
        if !self.capabilities.delete {
            return Err("delete capability disabled".into());
        }
        if !confirmed {
            return Err("explicit delete confirmation required".into());
        }
        if self
            .entries
            .get(index)
            .is_none_or(|e| e.name != expected_name)
        {
            return Err("delete selection changed: confirmation invalidated".into());
        }
        let (_path, _meta) = self.path_for(index)?;
        #[cfg(target_os = "linux")]
        {
            let (parent, pinned) = self.anchored_parent_for(index)?;
            mutate_verified_at(&parent, &pinned, &self.entries[index], None)?;
        }
        #[cfg(not(target_os = "linux"))]
        if _meta.is_dir() {
            fs::remove_dir(_path).map_err(|e| err("remove empty directory", e))?;
        } else {
            fs::remove_file(_path).map_err(|e| err("remove file", e))?;
        }
        self.refresh()?;
        self.message = format!("Deleted {expected_name}");
        Ok(())
    }
}
// Atomic NOREPLACE prevents an attacker or concurrent process from creating
// a target between the user confirmation and the rename system call.
#[cfg(target_os = "linux")]
fn rename_between_at(
    old_dir: &fs::File,
    old: &str,
    new_dir: &fs::File,
    new: &str,
) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint};
    unsafe extern "C" {
        fn renameat2(
            olddirfd: c_int,
            oldpath: *const c_char,
            newdirfd: c_int,
            newpath: *const c_char,
            flags: c_uint,
        ) -> c_int;
    }
    let old = CString::new(old)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in old path"))?;
    let new = CString::new(new)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in new path"))?;
    const RENAME_NOREPLACE: c_uint = 1;
    let rc = unsafe {
        renameat2(
            old_dir.as_raw_fd(),
            old.as_ptr(),
            new_dir.as_raw_fd(),
            new.as_ptr(),
            RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "linux"))]
fn rename_without_replacing(_old: &Path, _new: &Path) -> io::Result<()> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "atomic no-replace rename unsupported",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};
    fn temp() -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "nuraloumi-file-manager-{}-{stamp}",
            std::process::id()
        ));
        fs::create_dir(&root).unwrap();
        root
    }
    #[test]
    fn root_navigation_and_caps() {
        let root = temp();
        fs::create_dir(root.join("folder")).unwrap();
        fs::write(root.join("file.txt"), b"hello").unwrap();
        let mut b = Browser::new(&root, Capabilities::default()).unwrap();
        assert_eq!(b.entries.len(), 2);
        assert!(!b.parent().unwrap());
        let folder = b.entries.iter().position(|e| e.directory).unwrap();
        assert!(b.enter(folder).unwrap());
        assert_eq!(b.entries.len(), 0);
        assert!(b.parent().unwrap());
        assert_eq!(b.current, b.root);
        assert!(b.rename(1, "new", true).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn pinned_parent_remains_authority_after_root_path_is_replaced() {
        let root = temp();
        let relocated = root.with_extension("relocated");
        fs::write(root.join("target"), b"within pinned root").unwrap();
        let b = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        let expected = b.entries[0].clone();
        let (parent, pinned) = b.anchored_parent_for(0).unwrap();
        fs::rename(&root, &relocated).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("target"), b"new path must remain intact").unwrap();
        mutate_verified_at(&parent, &pinned, &expected, None).unwrap();
        assert!(!relocated.join("target").exists());
        assert_eq!(
            fs::read(root.join("target")).unwrap(),
            b"new path must remain intact"
        );
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(relocated).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn interrupted_staging_is_visible_as_recovery_warning() {
        let root = temp();
        let stage = root.join(".nuraloumi-stage-interrupted");
        fs::create_dir(&stage).unwrap();
        fs::write(stage.join("entry"), b"not lost").unwrap();
        let browser = Browser::new(&root, Capabilities::default()).unwrap();
        assert!(browser.entries.is_empty());
        assert!(browser.message.contains("WARNING"));
        assert!(browser.message.contains("interrupted mutation staging"));
        assert_eq!(fs::read(stage.join("entry")).unwrap(), b"not lost");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn symlink_swap_after_last_check_preserves_external_file() {
        use std::os::unix::fs::symlink;
        let root = temp();
        let external = temp();
        fs::write(external.join("valuable"), b"keep me").unwrap();
        fs::write(root.join("target"), b"old").unwrap();
        let b = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        let expected = b.entries[0].clone();
        let (parent, pinned) = b.anchored_parent_for(0).unwrap();
        fs::rename(root.join("target"), root.join("preserved-original")).unwrap();
        symlink(external.join("valuable"), root.join("target")).unwrap();
        let error = mutate_verified_at(&parent, &pinned, &expected, None).unwrap_err();
        assert!(error.contains("changed after validation"), "{error}");
        assert_eq!(fs::read(external.join("valuable")).unwrap(), b"keep me");
        assert!(fs::symlink_metadata(root.join("target"))
            .unwrap()
            .file_type()
            .is_symlink());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(external).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn directory_swap_before_delete_preserves_replacement_contents() {
        let root = temp();
        fs::create_dir(root.join("target")).unwrap();
        let b = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        let expected = b.entries[0].clone();
        let (parent, pinned) = b.anchored_parent_for(0).unwrap();
        fs::rename(root.join("target"), root.join("preserved-original")).unwrap();
        fs::create_dir(root.join("target")).unwrap();
        fs::write(root.join("target/important"), b"preserve").unwrap();
        let error = mutate_verified_at(&parent, &pinned, &expected, None).unwrap_err();
        assert!(error.contains("changed after validation"), "{error}");
        assert_eq!(
            fs::read(root.join("target/important")).unwrap(),
            b"preserve"
        );
        assert!(root.join("preserved-original").is_dir());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn delete_leaf_swap_after_last_inode_check_cannot_delete_replacement() {
        let root = temp();
        fs::write(root.join("target"), b"original").unwrap();
        let b = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        let selected = b.entries[0].clone();
        // The original implementation had an unavoidable scheduling point
        // between the final anchored_parent_for() validation and unlinkat().
        let (parent, pinned) = b.anchored_parent_for(0).unwrap();
        fs::rename(root.join("target"), root.join("preserved-original")).unwrap();
        fs::write(root.join("target"), b"replacement").unwrap();
        let error = mutate_verified_at(&parent, &pinned, &selected, None).unwrap_err();
        assert!(error.contains("changed after validation"), "{error}");
        assert_eq!(fs::read(root.join("target")).unwrap(), b"replacement");
        assert_eq!(
            fs::read(root.join("preserved-original")).unwrap(),
            b"original"
        );
        assert!(!fs::read_dir(&root).unwrap().flatten().any(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with(".nuraloumi-stage-")));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn rename_leaf_swap_after_last_inode_check_cannot_publish_replacement() {
        let root = temp();
        fs::write(root.join("target"), b"original").unwrap();
        let b = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        let selected = b.entries[0].clone();
        let (parent, pinned) = b.anchored_parent_for(0).unwrap();
        fs::rename(root.join("target"), root.join("preserved-original")).unwrap();
        fs::write(root.join("target"), b"replacement").unwrap();
        let error = mutate_verified_at(&parent, &pinned, &selected, Some("renamed")).unwrap_err();
        assert!(error.contains("changed after validation"), "{error}");
        assert_eq!(fs::read(root.join("target")).unwrap(), b"replacement");
        assert_eq!(
            fs::read(root.join("preserved-original")).unwrap(),
            b"original"
        );
        assert!(!root.join("renamed").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn successful_delete_and_rename_cleanup_staging() {
        let root = temp();
        fs::write(root.join("a"), b"first").unwrap();
        fs::write(root.join("b"), b"second").unwrap();
        let mut b = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        let a = b.entries.iter().position(|e| e.name == "a").unwrap();
        b.rename(a, "renamed", true).unwrap();
        assert_eq!(fs::read(root.join("renamed")).unwrap(), b"first");
        let b_index = b.entries.iter().position(|e| e.name == "b").unwrap();
        b.delete(b_index, "b", true).unwrap();
        assert!(!root.join("b").exists());
        assert!(!fs::read_dir(&root).unwrap().flatten().any(|e| e
            .file_name()
            .to_string_lossy()
            .starts_with(".nuraloumi-stage-")));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn traversal_symlink_and_non_utf8() {
        assert!(!valid_name("../escape"));
        assert!(!valid_name(".."));
        assert!(!valid_name("a/b"));
        let root = temp();
        let outside = temp();
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStringExt;
            use std::os::unix::fs::symlink;
            symlink(&outside, root.join("outside")).unwrap();
            fs::write(
                root.join(std::ffi::OsString::from_vec(vec![0xff, 0xfe])),
                b"xx",
            )
            .unwrap();
        }
        let mut b = Browser::new(&root, Capabilities::default()).unwrap();
        assert!(b.entries.is_empty());
        b.refresh().unwrap();
        assert!(b.entries.is_empty());
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(outside).unwrap();
    }
    #[test]
    fn rename_collision_confirmation_and_delete() {
        let root = temp();
        fs::write(root.join("a.txt"), b"a").unwrap();
        fs::write(root.join("b.txt"), b"b").unwrap();
        fs::create_dir(root.join("occupied")).unwrap();
        fs::write(root.join("occupied").join("inside"), b"nested").unwrap();
        let mut b = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        let index = b.entries.iter().position(|e| e.name == "a.txt").unwrap();
        assert!(b.rename(index, "new.txt", false).is_err());
        assert!(b.rename(index, "b.txt", true).is_err());
        b.rename(index, "new.txt", true).unwrap();
        assert!(root.join("new.txt").exists());
        let index = b.entries.iter().position(|e| e.name == "new.txt").unwrap();
        assert!(b.delete(index, "new.txt", false).is_err());
        assert!(b.delete(index, "b.txt", true).is_err());
        b.delete(index, "new.txt", true).unwrap();
        let occupied = b.entries.iter().position(|e| e.name == "occupied").unwrap();
        assert!(b.delete(occupied, "occupied", true).is_err());
        assert!(root.join("occupied/inside").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn directory_scan_is_exhaustive_even_for_hidden_entries() {
        let root = temp();
        for i in 0..4097 {
            fs::write(root.join(format!(".hidden-{i}")), b"x").unwrap();
        }
        let browser = Browser::new(&root, Capabilities::default()).unwrap();
        assert!(browser.entries.is_empty());
        assert!(!browser.has_more);
        assert_eq!(browser.scanned, 4097);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn oversized_visible_directory_is_truncated_but_usable() {
        let root = temp();
        for i in 0..MAX_VISIBLE_ENTRIES + 12 {
            fs::write(root.join(format!("file-{i:04}")), b"a").unwrap();
        }
        let mut b = Browser::new(&root, Capabilities::default()).unwrap();
        assert_eq!(b.entries.len(), MAX_VISIBLE_ENTRIES);
        assert!(b.has_more);
        assert_eq!(b.entries[0].name, "file-0000");
        assert_eq!(
            b.entries.last().unwrap().name,
            format!("file-{:04}", MAX_VISIBLE_ENTRIES - 1)
        );
        assert!(!b.has_previous_batch());
        assert!(b.next_batch().unwrap());
        assert_eq!(b.batch_number(), 2);
        assert_eq!(b.entries.len(), 12);
        assert_eq!(b.entries[0].name, "file-0512");
        assert_eq!(b.entries.last().unwrap().name, "file-0523");
        assert!(!b.has_more);
        assert!(b.previous_batch().unwrap());
        assert_eq!(b.batch_number(), 1);
        assert_eq!(b.entries[0].name, "file-0000");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn pinned_root_relative_mutations_cannot_follow_swapped_root_path() {
        let root = temp();
        let moved = root.with_extension("moved");
        fs::write(root.join("inside"), "inside").unwrap();
        let browser = Browser::new(
            &root,
            Capabilities {
                rename: true,
                delete: true,
            },
        )
        .unwrap();
        // Pin the original directory; an attacker then moves that directory
        // and replaces its pathname with an unrelated directory.
        let fd = browser.anchored_parent().unwrap();
        fs::rename(&root, &moved).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("inside"), "outside").unwrap();
        rename_between_at(&fd, "inside", &fd, "renamed").unwrap();
        assert_eq!(fs::read(root.join("inside")).unwrap(), b"outside");
        assert_eq!(fs::read(moved.join("renamed")).unwrap(), b"inside");
        delete_at(&fd, "renamed", false).unwrap();
        assert_eq!(fs::read(root.join("inside")).unwrap(), b"outside");
        fs::remove_dir_all(root).unwrap();
        fs::remove_dir_all(moved).unwrap();
    }
    #[test]
    fn safe_unusual_filename_and_changed_inode_are_checked() {
        let root = temp();
        fs::write(root.join("-; tricky name.txt"), b"one").unwrap();
        let b = Browser::new(&root, Capabilities::default()).unwrap();
        assert_eq!(b.entries[0].name, "-; tricky name.txt");
        assert!(b.regular_file(0).unwrap().ends_with("-; tricky name.txt"));
        fs::remove_file(root.join("-; tricky name.txt")).unwrap();
        assert!(b.regular_file(0).is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
