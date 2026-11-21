#[path = "../src/browser/supervisor.rs"]
mod supervisor;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf, thread, time::Duration};
use supervisor::{Session, State, Supervisor};

fn fixture_dir() -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "nura-supervisor-{}-{:?}",
        std::process::id(),
        thread::current().id()
    ));
    fs::create_dir(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn shell(script: &str) -> Supervisor {
    Supervisor::new("/bin/sh".into(), vec!["-c".into(), script.into()])
}
#[test]
fn startup_is_not_success_without_authenticated_ready_signal() {
    let mut s = shell("sleep 10");
    s.configure(
        0,
        Duration::from_millis(100),
        Duration::from_millis(20),
        Duration::ZERO,
    );
    s.start().unwrap();
    assert_eq!(s.state(), &State::Starting);
    s.poll(|| false);
    assert_eq!(s.state(), &State::Starting);
    thread::sleep(Duration::from_millis(120));
    s.poll(|| false);
    assert!(matches!(s.state(), State::Failed(message) if message.contains("timeout")));
}
#[test]
fn ready_requires_probe_and_explicit_stop() {
    let mut s = shell("sleep 10");
    s.start().unwrap();
    s.poll(|| true);
    assert_eq!(s.state(), &State::Ready);
    s.stop();
    assert_eq!(s.state(), &State::Stopped);
}
#[test]
fn crash_is_bounded_and_observable() {
    let mut s = shell("exit 7");
    s.configure(
        1,
        Duration::from_secs(2),
        Duration::from_millis(10),
        Duration::ZERO,
    );
    s.start().unwrap();
    for _ in 0..60 {
        s.poll(|| false);
        if matches!(s.state(), State::Failed(_)) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(matches!(s.state(), State::Failed(message) if message.contains("exited")));
}
#[test]
fn restore_preserves_order_and_active_tab() {
    let dir = fixture_dir();
    let path = dir.join("session.json");
    let session = Session {
        tabs: vec![
            "https://example.org/a".into(),
            "https://example.org/b".into(),
            "about:blank".into(),
        ],
        active: 1,
    };
    session.save(&path).unwrap();
    assert_eq!(Session::load(&path).unwrap(), session);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
}
#[test]
fn malformed_session_fails_closed() {
    let dir = fixture_dir();
    let path = dir.join("session.json");
    fs::write(&path, r#"{"tabs":["javascript:alert(1)"],"active":0}"#).unwrap();
    assert!(Session::load(&path).is_err());
    fs::remove_file(path).unwrap();
    fs::remove_dir(dir).unwrap();
}

#[test]
fn remote_cli_uses_running_engine_without_wayland() {
    use std::{
        io::{BufRead, BufReader, Write},
        os::unix::net::UnixListener,
        process::Command,
    };

    let runtime = fixture_dir();
    let socket = runtime.join("nura-browser.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let child = || {
        let mut cli = Command::new(env!("CARGO_BIN_EXE_nuraloumi-browser"));
        cli.env("XDG_RUNTIME_DIR", &runtime)
            .env_remove("WAYLAND_DISPLAY")
            .arg("--remote");
        cli
    };
    assert_eq!(
        child()
            .args(["tab-select", "6"])
            .output()
            .unwrap()
            .status
            .code(),
        Some(2)
    );

    let worker = thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        let mut line = String::new();
        BufReader::new(&conn).read_line(&mut line).unwrap();
        let request: serde_json::Value = serde_json::from_str(&line).unwrap();
        assert_eq!(
            request,
            serde_json::json!({"command": "open", "argument": "https://example.org/"})
        );
        conn.write_all(b"{\"ok\":true,\"result\":{}}\n").unwrap();
    });
    let output = child()
        .args(["open", "https://example.org/"])
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    let response: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response, serde_json::json!({"ok": true, "result": {}}));
    worker.join().unwrap();

    fs::set_permissions(&socket, fs::Permissions::from_mode(0o666)).unwrap();
    let output = child().arg("uri").output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unsafe"));
    fs::remove_file(socket).unwrap();
    fs::remove_dir(runtime).unwrap();
}
