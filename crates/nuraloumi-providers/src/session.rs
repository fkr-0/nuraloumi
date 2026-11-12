use crate::command::{CommandLimits, CommandOutput, CommandRunner, CommandSpec};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use std::path::{Component, PathBuf};
use std::time::Duration;

const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(5), 16 * 1024);
const PROBE_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(2), 4 * 1024);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub meta: SnapshotMeta,
    pub destructive_actions_enabled: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    Suspend,
    Reboot,
    PowerOff,
    EndDesktop,
}

/// An explicitly bound *local* Sway session. No session ID or IPC path is
/// accepted from a menu row. The production constructor checks process and
/// filesystem ownership, rather than trusting environment strings alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedDesktopSession {
    session_id: String,
    vt: u32,
    uid: u32,
    sway_pid: u32,
    sway_socket: PathBuf,
}

impl OwnedDesktopSession {
    pub fn from_environment() -> Result<Self, ProviderError> {
        if ["SSH_CONNECTION", "SSH_CLIENT", "SSH_TTY"]
            .iter()
            .any(|name| env::var_os(name).is_some())
        {
            return Err(ProviderError::permission(
                "refusing desktop exit from an SSH environment",
            ));
        }
        if env::var("XDG_SESSION_TYPE").ok().as_deref() != Some("wayland") {
            return Err(ProviderError::unavailable(
                "desktop exit requires a local Wayland session",
            ));
        }
        let session_id = env::var("XDG_SESSION_ID")
            .map_err(|_| ProviderError::unavailable("XDG_SESSION_ID is missing"))?;
        if session_id.is_empty()
            || session_id.len() > 64
            || !session_id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(ProviderError::parse("invalid XDG_SESSION_ID"));
        }
        let vt = env::var("XDG_VTNR")
            .map_err(|_| ProviderError::unavailable("XDG_VTNR is missing"))?
            .parse::<u32>()
            .map_err(|_| ProviderError::parse("invalid XDG_VTNR"))?;
        if !(2..=63).contains(&vt) {
            return Err(ProviderError::permission(
                "desktop exit requires an owned graphical VT other than VT1",
            ));
        }
        let uid = fs::metadata("/proc/self")
            .map_err(|err| ProviderError::from_io("reading current process owner", &err))?
            .uid();

        let runtime = PathBuf::from(
            env::var_os("XDG_RUNTIME_DIR")
                .ok_or_else(|| ProviderError::unavailable("XDG_RUNTIME_DIR is missing"))?,
        );
        let socket = PathBuf::from(
            env::var_os("SWAYSOCK")
                .ok_or_else(|| ProviderError::unavailable("SWAYSOCK is missing"))?,
        );
        if !runtime.is_absolute()
            || !socket.is_absolute()
            || runtime
                .components()
                .any(|part| matches!(part, Component::ParentDir))
            || socket
                .components()
                .any(|part| matches!(part, Component::ParentDir))
            || socket.parent() != Some(runtime.as_path())
        {
            return Err(ProviderError::permission(
                "Sway IPC socket is outside the session runtime directory",
            ));
        }
        let runtime_meta = fs::symlink_metadata(&runtime)
            .map_err(|err| ProviderError::from_io("reading runtime directory", &err))?;
        let socket_meta = fs::symlink_metadata(&socket)
            .map_err(|err| ProviderError::from_io("reading Sway socket", &err))?;
        if !runtime_meta.is_dir()
            || runtime_meta.uid() != uid
            || !socket_meta.file_type().is_socket()
            || socket_meta.uid() != uid
        {
            return Err(ProviderError::permission(
                "Sway IPC ownership or socket type mismatch",
            ));
        }
        let name = socket
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");
        let prefix = format!("sway-ipc.{uid}.");
        let pid = name
            .strip_prefix(&prefix)
            .and_then(|tail| tail.strip_suffix(".sock"))
            .and_then(|text| text.parse::<u32>().ok())
            .filter(|pid| *pid > 1)
            .ok_or_else(|| ProviderError::parse("Sway IPC name must contain the owned PID"))?;
        let proc_dir = PathBuf::from(format!("/proc/{pid}"));
        let proc_owner = fs::metadata(&proc_dir)
            .map_err(|err| ProviderError::from_io("reading compositor process", &err))?;
        let comm = fs::read_to_string(proc_dir.join("comm"))
            .map_err(|err| ProviderError::from_io("reading compositor process name", &err))?;
        if proc_owner.uid() != uid || !matches!(comm.trim(), "sway" | "sl101-sway") {
            return Err(ProviderError::permission(
                "Sway IPC process is not owned Sway",
            ));
        }
        // The SL101 desktop supervisor can rename Sway to sl101-sway.
        // Require the executable to be Sway itself, not an arbitrary process
        // using the expected IPC socket basename.
        let executable = fs::read_link(proc_dir.join("exe"))
            .map_err(|err| ProviderError::from_io("reading compositor executable", &err))?;
        if !matches!(
            executable.file_name().and_then(|name| name.to_str()),
            Some("sway" | "sl101-sway")
        ) {
            return Err(ProviderError::permission(
                "Sway IPC process executable mismatch",
            ));
        }
        Ok(Self {
            session_id,
            vt,
            uid,
            sway_pid: pid,
            sway_socket: socket,
        })
    }
}

pub struct SessionProvider<R> {
    runner: R,
    destructive_actions_enabled: bool,
    suspend_actions_enabled: bool,
    /// Independent positive qualification; never inferred from the power flag.
    /// The UI does not set this until a freeze-only logind policy is qualified.
    freeze_only_suspend_qualified: bool,
    desktop_session: Option<OwnedDesktopSession>,
    timestamp_override: Option<u64>,
}

impl<R> SessionProvider<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            destructive_actions_enabled: false,
            suspend_actions_enabled: false,
            freeze_only_suspend_qualified: false,
            desktop_session: None,
            timestamp_override: None,
        }
    }

    pub fn with_destructive_actions(mut self, enabled: bool) -> Self {
        self.destructive_actions_enabled = enabled;
        self
    }

    pub fn with_suspend_actions(mut self, enabled: bool) -> Self {
        self.suspend_actions_enabled = enabled;
        self
    }

    // No public setter for freeze-only qualification exists: a caller flag
    // cannot assert the kernel/DTB and effective logind policy are safe.
    // Only controlled, non-destructive fixtures currently exercise the
    // positive path. All live suspend requests remain dry-run.

    pub fn with_desktop_session(mut self, session: OwnedDesktopSession) -> Self {
        self.desktop_session = Some(session);
        self
    }

    pub fn with_timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp_override = timestamp;
        self
    }
}

impl<R> Provider for SessionProvider<R> {
    type Snapshot = SessionSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let mut issues = Vec::new();
        if !self.destructive_actions_enabled {
            issues.push("destructive session actions disabled by caller capability".to_owned());
        }
        if !self.suspend_actions_enabled || !self.freeze_only_suspend_qualified {
            issues.push("suspend requires separate freeze-only qualification".to_owned());
        }
        if self.desktop_session.is_none() {
            issues.push("end desktop requires a verified owned Sway/VT session".to_owned());
        }
        Ok(SessionSnapshot {
            meta: SnapshotMeta::new(
                timestamp_ms(self.timestamp_override),
                Health::Healthy,
                false,
                "logind-session",
            ),
            destructive_actions_enabled: self.destructive_actions_enabled,
            issues,
        })
    }
}

fn checked_run<R: CommandRunner>(
    runner: &mut R,
    spec: CommandSpec,
    limits: CommandLimits,
) -> Result<CommandOutput, ProviderError> {
    let result = runner.run(&spec, limits)?;
    if result.status != 0 {
        return Err(ProviderError::backend(format!(
            "backend '{}' exited with status {}",
            spec.program, result.status
        )));
    }
    Ok(result)
}

fn checked_session_properties(
    text: &str,
    session: &OwnedDesktopSession,
) -> Result<String, ProviderError> {
    let mut fields = BTreeMap::new();
    for line in text.lines() {
        let (key, value) = line
            .split_once('=')
            .ok_or_else(|| ProviderError::parse("invalid logind session property"))?;
        if fields.insert(key, value).is_some() {
            return Err(ProviderError::parse("duplicate logind session property"));
        }
    }
    let required = [
        ("Active", "yes"),
        ("Remote", "no"),
        ("Type", "wayland"),
        ("Class", "user"),
        ("Seat", "seat0"),
        ("VTNr", &*session.vt.to_string()),
        ("User", &*session.uid.to_string()),
    ];
    if required
        .iter()
        .any(|(key, expected)| fields.get(key) != Some(expected))
    {
        return Err(ProviderError::permission(
            "logind does not own an active local graphical session on the expected VT",
        ));
    }
    if fields.get("TTY") != Some(&format!("tty{}", session.vt).as_str()) {
        return Err(ProviderError::permission(
            "logind TTY disagrees with the graphical VT",
        ));
    }
    let scope = fields.get("Scope").copied().unwrap_or("");
    if !scope.starts_with("session-")
        || !scope.ends_with(".scope")
        || scope.contains('/')
        || scope.len() > 100
    {
        return Err(ProviderError::permission(
            "logind session scope is not verifiable",
        ));
    }
    Ok(scope.to_owned())
}

fn end_owned_desktop<R: CommandRunner>(
    runner: &mut R,
    session: &OwnedDesktopSession,
) -> Result<ActionResult, ProviderError> {
    // Do not use terminate-session/kill-session: an unrelated SSH login or
    // process can outlive the desktop. Only this validated Sway IPC is told to exit.
    let properties = checked_run(
        runner,
        CommandSpec::new("loginctl").args([
            "show-session",
            &session.session_id,
            "-p",
            "Active",
            "-p",
            "Remote",
            "-p",
            "Type",
            "-p",
            "Class",
            "-p",
            "Seat",
            "-p",
            "VTNr",
            "-p",
            "User",
            "-p",
            "TTY",
            "-p",
            "Scope",
        ]),
        PROBE_LIMITS,
    )?;
    let scope = checked_session_properties(&properties.stdout, session)?;
    // The socket owner alone is insufficient: it could belong to a second
    // Sway process of the same user. Verify the compositor's logind scope.
    let cgroup = checked_run(
        runner,
        CommandSpec::new("cat").arg(format!("/proc/{}/cgroup", session.sway_pid)),
        PROBE_LIMITS,
    )?;
    let in_scope = cgroup.stdout.lines().any(|line| {
        line.split_once(':')
            .and_then(|(_, rest)| rest.split_once(':'))
            .is_some_and(|(_, path)| path.split('/').any(|segment| segment == scope))
    });
    if !in_scope {
        return Err(ProviderError::permission(
            "Sway process is not in the owned logind session scope",
        ));
    }
    let active_vt = checked_run(runner, CommandSpec::new("fgconsole"), PROBE_LIMITS)?;
    if active_vt.stdout.trim() != session.vt.to_string() {
        return Err(ProviderError::permission(
            "active VT is not the owned Sway VT",
        ));
    }
    let self_uid = checked_run(runner, CommandSpec::new("id").arg("-u"), PROBE_LIMITS)?;
    if self_uid.stdout.trim() != session.uid.to_string() {
        return Err(ProviderError::permission(
            "process UID differs from Sway session owner",
        ));
    }
    // Recheck ownership immediately before the exit IPC request in production.
    // A stale socket does not permit an unrelated session to be killed.
    checked_run(
        runner,
        CommandSpec::new("swaymsg")
            .arg("-s")
            .arg(session.sway_socket.to_string_lossy().into_owned())
            .arg("exit"),
        ACTION_LIMITS,
    )?;
    checked_run(runner, CommandSpec::new("chvt").arg("1"), ACTION_LIMITS)?;
    Ok(ActionResult::executed(
        "owned Sway desktop exited; VT1 requested",
    ))
}

impl<R: CommandRunner> ActionProvider<SessionAction> for SessionProvider<R> {
    fn execute(&mut self, action: SessionAction) -> Result<ActionResult, ProviderError> {
        if !self.destructive_actions_enabled {
            return Ok(ActionResult::dry_run(
                "destructive session action disabled by caller capability",
            ));
        }

        if action == SessionAction::Suspend && !self.suspend_actions_enabled {
            return Ok(ActionResult::dry_run(
                "suspend action disabled by separate caller capability",
            ));
        }
        if action == SessionAction::Suspend && !self.freeze_only_suspend_qualified {
            return Ok(ActionResult::dry_run(
                "sleep requires independently qualified freeze-only logind capability",
            ));
        }

        if action == SessionAction::EndDesktop {
            return match self.desktop_session.as_ref() {
                Some(session) => end_owned_desktop(&mut self.runner, session),
                None => Ok(ActionResult::dry_run(
                    "end desktop requires an owned and verified Sway session",
                )),
            };
        }

        let verb = match action {
            SessionAction::Suspend => "suspend",
            SessionAction::Reboot => "reboot",
            SessionAction::PowerOff => "poweroff",
            SessionAction::EndDesktop => unreachable!(),
        };
        checked_run(
            &mut self.runner,
            CommandSpec::new("loginctl").arg(verb),
            ACTION_LIMITS,
        )?;
        Ok(ActionResult::executed("session action submitted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};
    fn output(text: &str) -> CommandOutput {
        CommandOutput {
            status: 0,
            stdout: text.into(),
            stderr: String::new(),
        }
    }
    fn session() -> OwnedDesktopSession {
        OwnedDesktopSession {
            session_id: "c2".into(),
            vt: 2,
            uid: 1000,
            sway_pid: 123,
            sway_socket: PathBuf::from("/run/user/1000/sway-ipc.1000.123.sock"),
        }
    }
    fn properties() -> &'static str {
        "Active=yes\nRemote=no\nType=wayland\nClass=user\nSeat=seat0\nVTNr=2\nUser=1000\nTTY=tty2\nScope=session-c2.scope\n"
    }
    fn desktop_runner(props: &str, fg: &str, uid: &str) -> FixtureCommandRunner {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("loginctl").args([
                "show-session",
                "c2",
                "-p",
                "Active",
                "-p",
                "Remote",
                "-p",
                "Type",
                "-p",
                "Class",
                "-p",
                "Seat",
                "-p",
                "VTNr",
                "-p",
                "User",
                "-p",
                "TTY",
                "-p",
                "Scope",
            ]),
            output(props),
        );
        runner.insert(
            CommandSpec::new("cat").arg("/proc/123/cgroup"),
            output("0::/user.slice/session-c2.scope\n"),
        );
        runner.insert(CommandSpec::new("fgconsole"), output(fg));
        runner.insert(CommandSpec::new("id").arg("-u"), output(uid));
        runner.insert(
            CommandSpec::new("swaymsg").args([
                "-s",
                "/run/user/1000/sway-ipc.1000.123.sock",
                "exit",
            ]),
            output("success"),
        );
        runner.insert(CommandSpec::new("chvt").arg("1"), output(""));
        runner
    }

    #[test]
    fn power_action_is_dry_run_by_default() {
        let mut provider = SessionProvider::new(FixtureCommandRunner::default());
        for action in [
            SessionAction::Suspend,
            SessionAction::Reboot,
            SessionAction::PowerOff,
            SessionAction::EndDesktop,
        ] {
            let result = provider.execute(action).unwrap();
            assert!(!result.executed && result.dry_run);
        }
    }

    #[test]
    fn suspend_needs_both_independent_gates() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(CommandSpec::new("loginctl").arg("suspend"), output(""));
        let mut provider = SessionProvider::new(runner)
            .with_destructive_actions(true)
            .with_suspend_actions(true);
        assert!(provider.execute(SessionAction::Suspend).unwrap().dry_run);
        provider.freeze_only_suspend_qualified = true; // fixture evidence only
        assert!(provider.execute(SessionAction::Suspend).unwrap().executed);
    }

    #[test]
    fn reboot_and_poweroff_have_exact_inhibitor_aware_argv() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(CommandSpec::new("loginctl").arg("reboot"), output(""));
        runner.insert(CommandSpec::new("loginctl").arg("poweroff"), output(""));
        let mut provider = SessionProvider::new(runner).with_destructive_actions(true);
        assert!(provider.execute(SessionAction::Reboot).unwrap().executed);
        assert!(provider.execute(SessionAction::PowerOff).unwrap().executed);
    }

    #[test]
    fn end_desktop_requires_owned_local_active_vt() {
        let mut provider = SessionProvider::new(desktop_runner(properties(), "2\n", "1000\n"))
            .with_destructive_actions(true)
            .with_desktop_session(session());
        assert!(
            provider
                .execute(SessionAction::EndDesktop)
                .unwrap()
                .executed
        );
        for (props, vt, uid) in [
            (properties(), "3\n", "1000\n"),
            (properties(), "2\n", "1001\n"),
            ("Active=no\nRemote=no\nType=wayland\nClass=user\nSeat=seat0\nVTNr=2\nUser=1000\nTTY=tty2\n", "2\n", "1000\n"),
            ("Active=yes\nRemote=yes\nType=wayland\nClass=user\nSeat=seat0\nVTNr=2\nUser=1000\nTTY=tty2\n", "2\n", "1000\n"),
            ("Active=yes\nRemote=no\nType=tty\nClass=user\nSeat=seat0\nVTNr=2\nUser=1000\nTTY=tty2\n", "2\n", "1000\n"),
        ] {
            let mut provider = SessionProvider::new(desktop_runner(props, vt, uid))
                .with_destructive_actions(true).with_desktop_session(session());
            assert!(provider.execute(SessionAction::EndDesktop).is_err());
        }
    }

    #[test]
    fn desktop_exit_is_dry_run_without_identity_even_if_power_enabled() {
        let mut provider =
            SessionProvider::new(FixtureCommandRunner::default()).with_destructive_actions(true);
        assert!(provider.execute(SessionAction::EndDesktop).unwrap().dry_run);
    }

    #[test]
    fn no_inhibitor_bypass_or_session_wide_kill_strings() {
        let source = include_str!("session.rs");
        assert!(!source.contains("CommandSpec::new(\"rtcwake\")"));
        assert!(!source.contains("CommandSpec::new(\"kill\")"));
        assert!(!source.contains("CommandSpec::new(\"loginctl\").arg(\"terminate-session\")"));
    }
}
