//! Compositor identity is observed from the running process, never requested config.
use std::{
    fs::File,
    io::{self, Read},
    path::Path,
};

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
            && maps.lines().any(|line| {
                line.contains("/usr/local/lib/sl101-grate-")
                    && line.ends_with("/libgallium-25.0.7.so")
            })
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

pub fn running_renderer() -> &'static str {
    let probe = || -> io::Result<&'static str> {
        let pid = String::from_utf8_lossy(&bounded(Path::new("/run/sl101-desktop.pid"), 32)?)
            .trim()
            .parse::<u32>()
            .map_err(|_| io::Error::other("invalid labwc pid"))?;
        let base = format!("/proc/{pid}");
        let name = bounded(Path::new(&format!("{base}/comm")), 128)?;
        if name != b"labwc\n" {
            return Err(io::Error::other("pid is not labwc"));
        }
        let stat = bounded(Path::new(&format!("{base}/stat")), 4096)?;
        let environment = bounded(Path::new(&format!("{base}/environ")), 65536)?;
        let maps = bounded(Path::new(&format!("{base}/maps")), 1048576)?;
        if stat != bounded(Path::new(&format!("{base}/stat")), 4096)? {
            // Scheduling counters change too; compare start-time and process identity instead.
            let current = bounded(Path::new(&format!("{base}/stat")), 4096)?;
            let start = |s: &[u8]| {
                String::from_utf8_lossy(s)
                    .rsplit_once(") ")
                    .and_then(|(_, rest)| rest.split_whitespace().nth(19).map(str::to_owned))
            };
            if start(&stat).is_none() || start(&stat) != start(&current) {
                return Err(io::Error::other("labwc changed during probe"));
            }
        }
        Ok(classify(&environment, &String::from_utf8_lossy(&maps)))
    };
    probe().unwrap_or("Unknown")
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
                "/usr/local/lib/sl101-grate-test/lib/libgallium-25.0.7.so"
            ),
            "GLES2"
        );
        assert_eq!(classify(b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=grate\0WLR_RENDERER_ALLOW_SOFTWARE=0\0", "/usr/local/lib/sl101-grate-test/lib/libgallium-25.0.7.so"), "Grate");
        assert_eq!(classify(b"WLR_RENDERER=gles2\0MESA_LOADER_DRIVER_OVERRIDE=grate\0WLR_RENDERER_ALLOW_SOFTWARE=0\0", "/usr/lib/libgallium-25.0.7.so"), "GLES2");
    }
}
