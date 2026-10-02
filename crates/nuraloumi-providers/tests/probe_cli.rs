use std::path::Path;
use std::process::Command;

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
