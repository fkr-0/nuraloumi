use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn temporary_fixture(name: &str) -> PathBuf {
    let root =
        std::env::temp_dir().join(format!("nuraloumi-probe-cli-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(root.join("commands")).expect("create temporary fixture");
    root
}

#[test]
fn fixture_probe_json_is_deterministic_and_complete() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/providers/basic");
    let executable = env!("CARGO_BIN_EXE_nuraloumi-probe");

    let run = || {
        Command::new(executable)
            .arg("--fixture")
            .arg(&fixture)
            .output()
            .expect("run nuraloumi-probe")
    };

    let first = run();
    let second = run();
    assert!(first.status.success(), "{:?}", first);
    assert!(second.status.success(), "{:?}", second);
    assert_eq!(first.stdout, second.stdout);

    let json = String::from_utf8(first.stdout).expect("probe output UTF-8");
    assert!(json.contains("\"schema\": \"nuraloumi.probe.v1\""));
    assert!(json.contains("\"capacity_percent\":73"));
    assert!(json.contains("\"ssid\":\"Home:Lab\""));
    assert!(json.contains("\"signal_percent\":82"));
    assert!(json.contains("\"volume_percent\":42"));
    assert!(json.contains("\"destructive_actions_enabled\":false"));
}

#[test]
fn unsafe_suspend_requires_destructive_enablement() {
    let executable = env!("CARGO_BIN_EXE_nuraloumi-probe");
    let output = Command::new(executable)
        .args(["--enable-unsafe-suspend", "action", "session", "suspend"])
        .output()
        .expect("run nuraloumi-probe");

    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).expect("probe stderr UTF-8");
    assert!(stderr.contains("--enable-unsafe-suspend requires --enable-destructive"));
}

#[test]
fn destructive_enablement_alone_keeps_suspend_dry_run() {
    let fixture = temporary_fixture("suspend-dry-run");
    let executable = env!("CARGO_BIN_EXE_nuraloumi-probe");
    let output = Command::new(executable)
        .arg("--fixture")
        .arg(&fixture)
        .args(["--enable-destructive", "action", "session", "suspend"])
        .output()
        .expect("run nuraloumi-probe");
    let _ = fs::remove_dir_all(&fixture);

    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("probe stdout UTF-8"),
        "{\"executed\":false,\"dry_run\":true,\"message\":\"suspend action disabled by separate caller capability\"}\n"
    );
}

#[test]
fn explicit_unsafe_suspend_uses_exact_loginctl_fixture() {
    let fixture = temporary_fixture("suspend-explicit");
    fs::write(fixture.join("commands/001.argv"), "loginctl\nsuspend\n")
        .expect("write exact loginctl fixture");

    let executable = env!("CARGO_BIN_EXE_nuraloumi-probe");
    let output = Command::new(executable)
        .arg("--fixture")
        .arg(&fixture)
        .args([
            "--enable-destructive",
            "--enable-unsafe-suspend",
            "action",
            "session",
            "suspend",
        ])
        .output()
        .expect("run nuraloumi-probe");
    let _ = fs::remove_dir_all(&fixture);

    assert!(output.status.success(), "{:?}", output);
    assert_eq!(
        String::from_utf8(output.stdout).expect("probe stdout UTF-8"),
        "{\"executed\":true,\"dry_run\":false,\"message\":\"session action submitted\"}\n"
    );
}

#[test]
fn help_separates_power_and_suspend_enablement() {
    let executable = env!("CARGO_BIN_EXE_nuraloumi-probe");
    let output = Command::new(executable)
        .arg("--help")
        .output()
        .expect("run nuraloumi-probe help");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).expect("probe stdout UTF-8");
    assert!(stdout.contains("--enable-destructive"));
    assert!(stdout.contains("--enable-unsafe-suspend"));
    assert!(stdout.contains("Suspend remains dry-run"));
}
