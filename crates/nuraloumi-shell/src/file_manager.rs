//! Rooted, bounded quick-browser operations. No command strings or renderer effects.
use std::fs;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

// Scan/retain limits are independent: even oversized folders remain navigable.
// A truncated folder is displayed with a warning, not rejected wholesale.
pub const MAX_VISIBLE_ENTRIES: usize = 512;
pub const MAX_DIRECTORY_SCAN: usize = 4096;
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
#[derive(Debug, Clone, Copy, Default)]
pub struct Capabilities {
    pub rename: bool,
    pub delete: bool,
}
pub struct Browser {
    pub root: PathBuf,
    pub current: PathBuf,
    pub entries: Vec<Entry>,
    pub has_more: bool,
    pub scanned: usize,
    pub message: String,
    pub capabilities: Capabilities,
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
        let root = fs::canonicalize(root).map_err(|e| err("resolve browser root", e))?;
        if !root.is_dir() {
            return Err("browser root is not a directory".into());
        }
        let mut browse = Self {
            current: root.clone(),
            root,
            entries: Vec::new(),
            has_more: false,
            scanned: 0,
            message: "Read-only by default; R rename / D delete / O open-with".into(),
            capabilities,
        };
        browse.refresh()?;
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
    pub fn refresh(&mut self) -> Result<(), String> {
        self.ensure_current()?;
        let entries = fs::read_dir(&self.current).map_err(|e| err("list directory", e))?;
        let mut result = Vec::new();
        let mut rejected = 0usize;
        let mut scanned = 0usize;
        let mut truncated_scan = false;
        for e in entries {
            if scanned >= MAX_DIRECTORY_SCAN {
                truncated_scan = true;
                break;
            }
            scanned += 1;
            let entry = e.map_err(|e| err("read entry", e))?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                rejected += 1;
                continue;
            };
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
            result.push(Entry {
                name,
                directory: meta.is_dir(),
                len: meta.len(),
                #[cfg(unix)]
                device: meta.dev(),
                #[cfg(unix)]
                inode: meta.ino(),
            });
        }
        result.sort_by(|a, b| {
            b.directory.cmp(&a.directory).then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
                    .then_with(|| a.name.cmp(&b.name))
            })
        });
        let has_more = truncated_scan || result.len() > MAX_VISIBLE_ENTRIES;
        result.truncate(MAX_VISIBLE_ENTRIES);
        self.scanned = scanned;
        self.has_more = has_more;
        self.entries = result;
        self.message = if has_more {
            format!("Partial listing: {} shown, {scanned} scanned (limit {MAX_DIRECTORY_SCAN}); narrow directory", self.entries.len())
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
        if let Err(error) = self.refresh() {
            self.current = previous;
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
        if let Err(error) = self.refresh() {
            self.current = previous;
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
        let (path, meta) = self.path_for(index)?;
        if meta.is_dir() {
            fs::remove_dir(path).map_err(|e| err("remove empty directory", e))?;
        } else {
            fs::remove_file(path).map_err(|e| err("remove file", e))?;
        }
        self.refresh()?;
        self.message = format!("Deleted {expected_name}");
        Ok(())
    }
}
// Atomic NOREPLACE prevents an attacker or concurrent process from creating
// a target between the user confirmation and the rename system call.
#[cfg(target_os = "linux")]
fn rename_without_replacing(old: &Path, new: &Path) -> io::Result<()> {
    use std::ffi::CString;
    use std::os::raw::{c_char, c_int, c_uint};
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn renameat2(
            olddirfd: c_int,
            oldpath: *const c_char,
            newdirfd: c_int,
            newpath: *const c_char,
            flags: c_uint,
        ) -> c_int;
    }
    let old = CString::new(old.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in old path"))?;
    let new = CString::new(new.as_os_str().as_bytes())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "NUL in new path"))?;
    const AT_FDCWD: c_int = -100;
    const RENAME_NOREPLACE: c_uint = 1;
    let rc = unsafe {
        renameat2(
            AT_FDCWD,
            old.as_ptr(),
            AT_FDCWD,
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
    fn directory_scan_is_bounded_even_for_hidden_entries() {
        let root = temp();
        for i in 0..(MAX_DIRECTORY_SCAN + 1) {
            fs::write(root.join(format!(".hidden-{i}")), b"x").unwrap();
        }
        let browser = Browser::new(&root, Capabilities::default()).unwrap();
        assert!(browser.entries.is_empty());
        assert!(browser.has_more);
        assert_eq!(browser.scanned, MAX_DIRECTORY_SCAN);
        assert!(browser.message.contains("Partial listing"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn oversized_visible_directory_is_truncated_but_usable() {
        let root = temp();
        for i in 0..MAX_VISIBLE_ENTRIES + 12 {
            fs::write(root.join(format!("file-{i:04}")), b"a").unwrap();
        }
        let b = Browser::new(&root, Capabilities::default()).unwrap();
        assert_eq!(b.entries.len(), MAX_VISIBLE_ENTRIES);
        assert!(b.has_more);
        assert_eq!(b.entries[0].name, "file-0000");
        assert_eq!(
            b.entries.last().unwrap().name,
            format!("file-{:04}", MAX_VISIBLE_ENTRIES - 1)
        );
        fs::remove_dir_all(root).unwrap();
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
