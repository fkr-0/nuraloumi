//! Compositor identity is observed from the running process, never requested config.
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

/// Inspect the pathname remainder after the five fixed /proc/<pid>/maps fields.
/// Never identify a driver from a truncated path, inode, or unrelated substring.
fn maps_grate_gallium(maps: &str) -> bool {
    const LIB: &str = "/lib/libgallium-25.0.7.so";
    const CLEAN: &str = "/opt/grate-mesa25-clean/lib/arm-linux-gnueabihf/libgallium-25.0.7.so";
    fn hex(s: &str, min: usize, max: usize) -> Option<u64> {
        if !(min..=max).contains(&s.len())
            || !s
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return None;
        }
        u64::from_str_radix(s, 16).ok()
    }

    fn field(line: &str) -> Option<(&str, &str)> {
        let (head, rest) = line.split_once(' ')?;
        // The kernel emits ordinary ASCII spaces between the fixed fields.
        // Tabs and repeated spaces here are malformed, not path characters.
        if head.is_empty() || rest.is_empty() || rest.starts_with(' ') {
            return None;
        }
        Some((head, rest))
    }
    fn pathname(line: &str) -> Option<&str> {
        let (range, line) = field(line)?;
        let (start, end) = range.split_once('-')?;
        let start = hex(start, 8, 16)?;
        let end = hex(end, 8, 16)?;
        let (perms, line) = field(line)?;
        let (offset, line) = field(line)?;
        let (device, line) = field(line)?;
        let (major, minor) = device.split_once(':')?;
        let (inode, path) = line.split_once(' ')?;
        if start >= end
            || hex(offset, 8, 16).is_none()
            || hex(major, 2, 8).is_none()
            || hex(minor, 2, 8).is_none()
            || (inode.len() > 1 && inode.starts_with('0'))
            || inode.parse::<u64>().is_err()
            || !matches!(
                perms.as_bytes(),
                [b'r' | b'-', b'w' | b'-', b'x' | b'-', b'p' | b's']
            )
        {
            return None;
        }
        // The kernel may pad after the inode for a pathname column. Once the
        // path begins, all bytes—including spaces, tabs and "(deleted)"—are
        // part of the opaque name and must remain in the exact comparison.
        Some(path.trim_start_matches(' '))
    }
    fn installation_id_is_valid(id: &str) -> bool {
        !id.is_empty()
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
            && id != "."
            && id != ".."
    }
    maps.split('\n').any(|line| {
        let Some(path) = pathname(line) else {
            return false;
        };
        if path == CLEAN {
            return true;
        }
        if let Some(id) = path
            .strip_prefix("/opt/grate-mesa25-musl-")
            .and_then(|path| path.strip_suffix(LIB))
        {
            return installation_id_is_valid(id);
        }
        // Earlier SL101 installations used both a direct and a lib/ layout.
        path.strip_prefix("/usr/local/lib/sl101-grate-")
            .and_then(|path| {
                path.strip_suffix(LIB)
                    .or_else(|| path.strip_suffix("/libgallium-25.0.7.so"))
            })
            .is_some_and(installation_id_is_valid)
    })
}

pub fn classify(environment: &[u8], maps: &str) -> &'static str {
    let has = |value: &str| {
        environment
            .split(|b| *b == 0)
            .any(|entry| entry == value.as_bytes())
    };
    if has("WLR_RENDERER=pixman") {
        return "Pixman";
    }
    if has("WLR_RENDERER=gles2") {
        if has("MESA_LOADER_DRIVER_OVERRIDE=grate")
            && has("WLR_RENDERER_ALLOW_SOFTWARE=0")
            && maps_grate_gallium(maps)
        {
            return "Grate";
        }
        return "GLES2";
    }
    "Unknown"
}

fn bounded(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(io::Error::other("renderer probe exceeded limit"));
    }
    Ok(bytes)
}

fn is_supported_compositor(name: &[u8]) -> bool {
    // Keep this whitelist tied to the root-owned SL101 desktop service.
    // The separate sl101-sway executable is a no-file-capability copy of sway,
    // needed so musl honours the pinned native Grate EGL library paths.
    matches!(name, b"labwc\n" | b"sway\n" | b"sl101-sway\n")
}

fn probe_renderer_with(
    mut read: impl FnMut(&Path, u64) -> io::Result<Vec<u8>>,
    pid_file: &Path,
    proc_root: &Path,
) -> io::Result<&'static str> {
    let pid = String::from_utf8_lossy(&read(pid_file, 32)?)
        .trim()
        .parse::<u32>()
        .map_err(|_| io::Error::other("invalid desktop pid"))?;
    probe_renderer_pid_with(&mut read, pid, proc_root)
}

fn probe_renderer_pid_with(
    mut read: impl FnMut(&Path, u64) -> io::Result<Vec<u8>>,
    pid: u32,
    proc_root: &Path,
) -> io::Result<&'static str> {
    if pid == 0 {
        return Err(io::Error::other("invalid compositor pid"));
    }
    let base = proc_root.join(pid.to_string());
    let name = read(&base.join("comm"), 128)?;
    if !is_supported_compositor(&name) {
        return Err(io::Error::other("pid is not a supported SL101 compositor"));
    }
    let stat = read(&base.join("stat"), 4096)?;
    let environment = read(&base.join("environ"), 65536)?;
    let maps = read(&base.join("maps"), 1048576)?;
    // Scheduling counters can move; the start-time uniquely guards PID reuse.
    let current = read(&base.join("stat"), 4096)?;
    let start = |s: &[u8]| {
        String::from_utf8_lossy(s)
            .rsplit_once(") ")
            .and_then(|(_, rest)| rest.split_whitespace().nth(19).map(str::to_owned))
    };
    if start(&stat).is_none() || start(&stat) != start(&current) {
        return Err(io::Error::other("compositor changed during probe"));
    }
    Ok(classify(&environment, &String::from_utf8_lossy(&maps)))
}

/// The service PID file is optional in user-owned Sway sessions. When it is
/// missing or stale, accept an observed compositor only if it is the *sole*
/// supported compositor in /proc. Multiple sessions must not be guessed.
/// Candidate PID discovery and probing are separate for deterministic tests.
fn sole_compositor_with(
    candidates: impl IntoIterator<Item = u32>,
    mut read: impl FnMut(&Path, u64) -> io::Result<Vec<u8>>,
    proc_root: &Path,
) -> io::Result<&'static str> {
    let mut selected = None;
    for pid in candidates {
        if let Ok(comm) = read(&proc_root.join(pid.to_string()).join("comm"), 128) {
            if is_supported_compositor(&comm) && selected.replace(pid).is_some() {
                return Err(io::Error::other("ambiguous compositor sessions"));
            }
        }
    }
    probe_renderer_pid_with(
        &mut read,
        selected.ok_or_else(|| io::Error::other("no compositor"))?,
        proc_root,
    )
}

fn observed_compositor_pids(proc_root: &Path) -> io::Result<Vec<u32>> {
    // Bound /proc discovery even on unusually busy machines.
    let mut pids = Vec::new();
    for (index, entry) in std::fs::read_dir(proc_root)?.enumerate() {
        if index >= 8192 {
            return Err(io::Error::other("process enumeration exceeded limit"));
        }
        let entry = entry?;
        if let Some(pid) = entry
            .file_name()
            .to_str()
            .and_then(|value| value.parse().ok())
        {
            pids.push(pid);
        }
    }
    Ok(pids)
}

pub fn running_renderer() -> &'static str {
    let pid_file = Path::new("/run/sl101-desktop.pid");
    let proc_root = Path::new("/proc");
    if let Ok(renderer) = probe_renderer_with(bounded, pid_file, proc_root) {
        return renderer;
    }
    observed_compositor_pids(proc_root)
        .and_then(|pids| sole_compositor_with(pids, bounded, proc_root))
        .unwrap_or("Unknown")
}

/// Request an installed switch helper; completion is observed by a new session.
pub fn request_switch<R: crate::CommandRunner>(
    runner: &mut R,
    mode: &str,
    enabled: bool,
) -> Result<crate::ActionResult, crate::ProviderError> {
    if !enabled {
        return Err(crate::ProviderError::permission(
            "Renderer switching is disabled",
        ));
    }
    if !matches!(mode, "pixman" | "grate") {
        return Err(crate::ProviderError::parse("Invalid renderer mode"));
    }
    let output = runner.run(
        &crate::CommandSpec::new("sudo").args([
            "-n",
            "/usr/local/sbin/sl101-renderer-switch",
            mode,
        ]),
        crate::CommandLimits::new(std::time::Duration::from_secs(3), 4096),
    )?;
    if output.status != 0 {
        return Err(crate::ProviderError::backend(output.stderr));
    }
    Ok(crate::ActionResult::executed(output.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::{HashMap, HashSet},
        path::PathBuf,
    };

    // Environment and mapped library are from the coordinator's 2026-11-09
    // Sway observation; /proc/maps addresses in the fixture are illustrative.
    const SWAY_ENV: &[u8] =
        b"WLR_RENDERER=gles2\0WLR_RENDERER_ALLOW_SOFTWARE=0\0MESA_LOADER_DRIVER_OVERRIDE=grate\0";
    const SWAY_MAPS: &str =
        include_str!("../../../tests/fixtures/providers/renderer/sl101-sway-20261109.maps");

    fn maps(path: &str) -> String {
        format!("b6e00000-b6e60000 r-xp 00000000 b3:01 17985 {path}\n")
    }

    fn stat(start: u32) -> Vec<u8> {
        format!("23991 (sl101-sway) S {} {start}\n", ["0"; 18].join(" ")).into_bytes()
    }

    struct FixtureProc {
        entries: HashMap<PathBuf, Vec<u8>>,
        second_stat: Option<Vec<u8>>,
        permission_denied: HashSet<PathBuf>,
    }

    impl FixtureProc {
        fn measured() -> Self {
            let base = Path::new("/fixture/proc/23991");
            Self {
                entries: HashMap::from([
                    (PathBuf::from("/fixture/desktop.pid"), b"23991\n".to_vec()),
                    (base.join("comm"), b"sl101-sway\n".to_vec()),
                    (base.join("stat"), stat(12345)),
                    (base.join("environ"), SWAY_ENV.to_vec()),
                    (base.join("maps"), SWAY_MAPS.as_bytes().to_vec()),
                ]),
                second_stat: None,
                permission_denied: HashSet::new(),
            }
        }

        fn run(&self) -> &'static str {
            let mut stat_reads = 0;
            probe_renderer_with(
                |path, limit| {
                    if self.permission_denied.contains(path) {
                        return Err(io::Error::from(io::ErrorKind::PermissionDenied));
                    }
                    let data = if path == Path::new("/fixture/proc/23991/stat") {
                        stat_reads += 1;
                        if stat_reads == 2 {
                            self.second_stat.as_ref().or_else(|| self.entries.get(path))
                        } else {
                            self.entries.get(path)
                        }
                    } else {
                        self.entries.get(path)
                    }
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
                    if data.len() as u64 > limit {
                        return Err(io::Error::other("renderer probe exceeded limit"));
                    }
                    Ok(data.clone())
                },
                Path::new("/fixture/desktop.pid"),
                Path::new("/fixture/proc"),
            )
            .unwrap_or("Unknown")
        }
    }

    #[test]
    fn measured_sway_session_is_grate() {
        assert_eq!(classify(SWAY_ENV, SWAY_MAPS), "Grate");
        assert_eq!(FixtureProc::measured().run(), "Grate");
    }

    #[test]
    fn sole_sway_fallback_requires_unambiguous_live_process() {
        let fixture = FixtureProc::measured();
        let run = |pids| {
            sole_compositor_with(
                pids,
                |path, _| {
                    fixture
                        .entries
                        .get(path)
                        .cloned()
                        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))
                },
                Path::new("/fixture/proc"),
            )
        };
        assert_eq!(run(vec![23991]).unwrap(), "Grate");
        assert!(run(vec![]).is_err());
        assert!(run(vec![12]).is_err());
        assert_eq!(run(vec![12, 23991]).unwrap(), "Grate");
        assert!(run(vec![23991, 23991]).is_err());
    }

    #[test]
    fn accepts_all_pinned_and_legacy_gallium_layouts() {
        for path in [
            "/opt/grate-mesa25-musl-0052-20261108/lib/libgallium-25.0.7.so",
            "/opt/grate-mesa25-musl-prod70-20261109/lib/libgallium-25.0.7.so",
            "/opt/grate-mesa25-clean/lib/arm-linux-gnueabihf/libgallium-25.0.7.so",
            "/usr/local/lib/sl101-grate-deadbeef/lib/libgallium-25.0.7.so",
            "/usr/local/lib/sl101-grate-deadbeef/libgallium-25.0.7.so",
        ] {
            assert_eq!(classify(SWAY_ENV, &maps(path)), "Grate", "{path}");
        }
    }

    #[test]
    fn maps_requires_exact_pathname_column_and_install_layout() {
        for line in [
            "not a maps entry /opt/grate-mesa25-musl-prod70-20261109/lib/libgallium-25.0.7.so",
            "b6e00000-b6e60000 r-xp 0000 00:01 0 /tmp/lib.so /opt/grate-mesa25-musl-prod70-20261109/lib/libgallium-25.0.7.so",
            "b6e00000-b6e60000 r-xp 0000 00:01 0 /opt/grate-mesa25-musl-prod70-20261109/lib/libgallium-25.0.7.so.bak",
            "b6e00000-b6e60000 r-xp 0000 00:01 0 /opt/grate-mesa25-musl-a/b/lib/libgallium-25.0.7.so",
            "b6e00000-b6e60000 r-xp 0000 00:01 0 /opt/grate-mesa25-clean/lib/arm-linux-gnueabihf/libgallium-25.0.6.so",
            "b6e00000-b6e60000 r-xp 0000 00:01 0 /usr/local/lib/sl101-grate-/lib/libgallium-25.0.7.so",
        ] {
            assert_eq!(classify(SWAY_ENV, line), "GLES2", "{line}");
        }
    }

    #[test]
    fn maps_pathname_suffix_and_embedded_whitespace_never_alias_driver() {
        let driver = "/opt/grate-mesa25-musl-fixture/lib/libgallium-25.0.7.so";
        for path in [
            format!("{driver} (deleted)"),
            format!("{driver} extra"),
            format!("{driver}\tother"),
            format!("{driver}\r"),
            format!("{driver}\\012"),
            format!("/tmp/other {driver}"),
        ] {
            assert_eq!(classify(SWAY_ENV, &maps(&path)), "GLES2", "{path:?}");
        }
        // Padding after inode is not part of the name; content after the
        // first pathname byte must never be split or truncated.
        assert_eq!(
            classify(
                SWAY_ENV,
                &format!("b6e00000-b6e60000 r-xp 00000000 b3:01 17985      {driver}\n")
            ),
            "Grate"
        );
    }

    #[test]
    fn maps_rejects_noncanonical_headers_and_invalid_intervals() {
        let driver = "/opt/grate-mesa25-musl-fixture/lib/libgallium-25.0.7.so";
        for header in [
            "B6e00000-b6e60000 r-xp 00000000 b3:01 17985",
            "b6e00000-b6e60000 r-xp 00000000 B3:01 17985",
            "b6e00000-b6e60000 r-xp 00000000 b3:1 17985",
            "b6e00000-b6e60000 r-xp 00000000 b3:01 017985",
            "b6e00000-b6e60000 r-xp 00000000 b3:01 -1",
            "b6e60000-b6e00000 r-xp 00000000 b3:01 17985",
            "b6e00000-b6e00000 r-xp 00000000 b3:01 17985",
            "b6e00000-b6e60000 r-zp 00000000 b3:01 17985",
            "b6e00000-b6e60000  r-xp 00000000 b3:01 17985",
            "b6e00000-b6e60000\tr-xp 00000000 b3:01 17985",
            "b6e00000-b6e60000 r-xp 00000000 b3:01\t17985",
            "b6e00000-b6e60000 r-xp 0000000G b3:01 17985",
        ] {
            assert_eq!(
                classify(SWAY_ENV, &format!("{header} {driver}\n")),
                "GLES2",
                "{header}"
            );
        }
        // Anonymous entries are valid but cannot identify a driver.
        assert_eq!(
            classify(SWAY_ENV, "b6e00000-b6e60000 rw-p 00000000 00:00 0  \n"),
            "GLES2"
        );
    }

    #[test]
    fn gles2_requires_every_grate_precondition() {
        assert_eq!(classify(b"WLR_RENDERER=gles2\0", SWAY_MAPS), "GLES2");
        assert_eq!(
            classify(
                b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=grate\0",
                SWAY_MAPS
            ),
            "GLES2"
        );
        assert_eq!(classify(b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=grate\0WLR_RENDERER_ALLOW_SOFTWARE=1\0", SWAY_MAPS), "GLES2");
        assert_eq!(classify(b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=other\0WLR_RENDERER_ALLOW_SOFTWARE=0\0", SWAY_MAPS), "GLES2");
        assert_eq!(classify(b"WLR_RENDERER=pixman\0", SWAY_MAPS), "Pixman");
    }

    #[test]
    fn missing_unreadable_oversize_wrong_compositor_and_pid_reuse_are_unknown() {
        let mut fixture = FixtureProc::measured();
        fixture.entries.remove(Path::new("/fixture/desktop.pid"));
        assert_eq!(fixture.run(), "Unknown"); // missing PID file

        for path in ["comm", "stat", "environ", "maps"] {
            let mut fixture = FixtureProc::measured();
            fixture
                .entries
                .remove(Path::new(&format!("/fixture/proc/23991/{path}")));
            assert_eq!(fixture.run(), "Unknown", "missing {path}");
            let mut fixture = FixtureProc::measured();
            fixture
                .permission_denied
                .insert(PathBuf::from(format!("/fixture/proc/23991/{path}")));
            assert_eq!(fixture.run(), "Unknown", "unreadable {path}");
        }
        let mut fixture = FixtureProc::measured();
        fixture.entries.insert(
            PathBuf::from("/fixture/proc/23991/comm"),
            b"other-sway\n".to_vec(),
        );
        assert_eq!(fixture.run(), "Unknown");

        for (path, limit) in [
            ("/fixture/desktop.pid", 32),
            ("/fixture/proc/23991/comm", 128),
            ("/fixture/proc/23991/stat", 4096),
            ("/fixture/proc/23991/environ", 65536),
            ("/fixture/proc/23991/maps", 1048576),
        ] {
            let mut fixture = FixtureProc::measured();
            fixture
                .entries
                .insert(PathBuf::from(path), vec![b'x'; limit + 1]);
            assert_eq!(fixture.run(), "Unknown", "oversize {path}");
        }
        let mut fixture = FixtureProc::measured();
        fixture.second_stat = Some(stat(12346));
        assert_eq!(fixture.run(), "Unknown", "PID reused while probing");

        let mut fixture = FixtureProc::measured();
        fixture.second_stat = Some(stat(12345).into_iter().chain(*b" ").collect());
        assert_eq!(fixture.run(), "Grate", "scheduling counters may change");
    }

    #[test]
    fn bounded_read_rejects_oversize_real_file() {
        let path =
            std::env::temp_dir().join(format!("nuraloumi-renderer-bounded-{}", std::process::id()));
        std::fs::write(&path, [b'x'; 33]).unwrap();
        assert_eq!(bounded(&path, 32).unwrap_err().kind(), io::ErrorKind::Other);
        assert_eq!(bounded(&path, 33).unwrap().len(), 33);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn recognizes_supported_compositor_executable_names() {
        assert!(is_supported_compositor(b"labwc\n"));
        assert!(is_supported_compositor(b"sway\n"));
        assert!(is_supported_compositor(b"sl101-sway\n"));
        assert!(!is_supported_compositor(b"other-sway\n"));
        assert!(!is_supported_compositor(b"labwc "));
        assert!(!is_supported_compositor(b""));
    }
    #[test]
    fn switch_actions_require_authorization_and_exact_argv() {
        let mut runner = crate::FixtureCommandRunner::default();
        assert!(request_switch(&mut runner, "grate", false).is_err());
        assert!(request_switch(&mut runner, "grate; touch /tmp/x", true).is_err());
        runner.insert(
            crate::CommandSpec::new("sudo").args([
                "-n",
                "/usr/local/sbin/sl101-renderer-switch",
                "grate",
            ]),
            crate::CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "qualification owns the device".into(),
            },
        );
        assert!(request_switch(&mut runner, "grate", true)
            .unwrap_err()
            .message
            .contains("qualification owns"));
        runner.insert(
            crate::CommandSpec::new("sudo").args([
                "-n",
                "/usr/local/sbin/sl101-renderer-switch",
                "pixman",
            ]),
            crate::CommandOutput {
                status: 0,
                stdout: "Requested pixman".into(),
                stderr: String::new(),
            },
        );
        assert!(
            request_switch(&mut runner, "pixman", true)
                .unwrap()
                .executed
        );
    }
    #[test]
    fn labels_software_and_unknown() {
        assert_eq!(classify(b"WLR_RENDERER=pixman\0", ""), "Pixman");
        assert_eq!(classify(b"", ""), "Unknown");
    }
    #[test]
    fn gles2_is_not_necessarily_grate() {
        assert_eq!(
            classify(
                b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=grate\0",
                &maps("/usr/local/lib/sl101-grate-test/lib/libgallium-25.0.7.so")
            ),
            "GLES2"
        );
        assert_eq!(classify(b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=grate\0WLR_RENDERER_ALLOW_SOFTWARE=0\0", &maps("/usr/local/lib/sl101-grate-test/lib/libgallium-25.0.7.so")), "Grate");
        assert_eq!(classify(b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=grate\0WLR_RENDERER_ALLOW_SOFTWARE=0\0", &maps("/usr/lib/libgallium-25.0.7.so")), "GLES2");
    }
}
