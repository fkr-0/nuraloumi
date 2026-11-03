#[allow(dead_code)]
#[path = "../src/bin/config.rs"]
mod config_cli;
#[path = "../src/config_migration.rs"]
mod config_migration;

use config_migration::*;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

static TEST_ID: AtomicUsize = AtomicUsize::new(0);

struct FixtureDir(std::path::PathBuf);
impl FixtureDir {
    fn new() -> Self {
        let id = TEST_ID.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("nuraloumi-config-test-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        Self(path)
    }
    fn path(&self, filename: &str) -> std::path::PathBuf {
        self.0.join(filename)
    }
}
impl Drop for FixtureDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn toml_v0_migration_preserves_extensions_and_is_idempotent() {
    let path = Path::new("config.toml");
    let source = r#"panel_height = 50
future_level = 7
[surface.third_party]
color = "teal"
enabled = true
[providers]
custom_program = "fixture"
"#;
    let migrated = plan(path, source).unwrap();
    assert!(migrated.changed);
    assert_eq!(migrated.from_version, 0);
    assert_eq!(migrated.to_version, 1);
    assert!(migrated
        .diagnostics
        .iter()
        .any(|d| d.code == "unknown_top_level"));
    let validated = prepare_reload(path, &migrated.output).unwrap();
    assert_eq!(validated.config.extra["future_level"].as_integer(), Some(7));
    assert_eq!(
        validated.config.surface["third_party"].extra["color"].as_str(),
        Some("teal")
    );
    assert_eq!(
        validated.config.providers.extra["custom_program"].as_str(),
        Some("fixture")
    );
    let again = plan(path, &migrated.output).unwrap();
    assert!(!again.changed);
    assert_eq!(again.output, migrated.output);
}

#[test]
fn json_v0_migration_keeps_extensible_sections() {
    let path = Path::new("config.json");
    let source =
        r#"{"panel_edge":"left","future_level":7,"surface":{"third_party":{"color":"teal"}}}"#;
    let proposal = plan(path, source).unwrap();
    let validated = prepare_reload(path, &proposal.output).unwrap();
    assert_eq!(validated.config.extra["future_level"].as_integer(), Some(7));
    assert_eq!(
        validated.config.surface["third_party"].extra["color"].as_str(),
        Some("teal")
    );
    assert_eq!(validated.config.config_version, 1);
}

#[test]
fn current_version_is_byte_for_byte_unchanged() {
    let source = "config_version = 1\npanel_height = 40\n";
    let planned = plan(Path::new("config.toml"), source).unwrap();
    assert!(!planned.changed);
    assert_eq!(planned.output, source);
}

#[test]
fn detects_future_version_invalid_schema_and_unknown_strict_fields() {
    let path = Path::new("config.toml");
    assert_eq!(
        plan(path, "config_version = 2").unwrap_err().code,
        "unsupported_version"
    );
    assert_eq!(
        plan(path, "panel_height = 0").unwrap_err().code,
        "validation_error"
    );
    assert_eq!(
        plan(path, "[providers.media]\nunknown = true")
            .unwrap_err()
            .code,
        "parse_error"
    );
    assert_eq!(
        plan(Path::new("config.yaml"), "config_version: 0")
            .unwrap_err()
            .code,
        "unsupported_format"
    );
    assert_eq!(
        plan(path, "config_version = -1").unwrap_err().code,
        "invalid_version"
    );
}

#[test]
fn explicit_write_is_atomic_and_requires_fresh_backup() {
    let fixture = FixtureDir::new();
    let path = fixture.path("config.toml");
    let input = "panel_height = 50\n";
    fs::write(&path, input).unwrap();
    let p = plan(&path, input).unwrap();
    let before = fs::read_to_string(&path).unwrap();
    assert_eq!(before, input);
    assert_eq!(write_explicit(&path, &p).unwrap(), WriteOutcome::Updated);
    assert_eq!(
        fs::read_to_string(fixture.path("config.toml.bak")).unwrap(),
        input
    );
    assert_eq!(
        prepare_reload(&path, &fs::read_to_string(&path).unwrap())
            .unwrap()
            .source_version,
        1
    );
    assert_eq!(
        write_explicit(&path, &p).unwrap_err().code,
        "concurrent_change"
    );
}

#[test]
fn already_existing_backup_and_concurrent_source_never_get_overwritten() {
    let fixture = FixtureDir::new();
    let path = fixture.path("config.json");
    fs::write(&path, "{}").unwrap();
    let p = plan(&path, "{}").unwrap();
    fs::write(fixture.path("config.json.bak"), "prior backup").unwrap();
    assert_eq!(write_explicit(&path, &p).unwrap_err().code, "backup_error");
    assert_eq!(fs::read_to_string(&path).unwrap(), "{}");
    assert_eq!(
        fs::read_to_string(fixture.path("config.json.bak")).unwrap(),
        "prior backup"
    );
    fs::remove_file(fixture.path("config.json.bak")).unwrap();
    fs::write(&path, "{ }").unwrap();
    assert_eq!(
        write_explicit(&path, &p).unwrap_err().code,
        "concurrent_change"
    );
    assert!(!fixture.path("config.json.bak").exists());
}

#[test]
fn cli_is_read_only_by_default_and_reports_warnings() {
    let fixture = FixtureDir::new();
    let path = fixture.path("config.toml");
    fs::write(&path, "future_setting = 2\n").unwrap();
    let args = vec![
        "migrate".into(),
        "--config".into(),
        path.display().to_string(),
    ];
    let output = config_cli::run(args).unwrap();
    assert!(output.contains("dry-run"));
    assert_eq!(fs::read_to_string(&path).unwrap(), "future_setting = 2\n");
    assert!(!fixture.path("config.toml.bak").exists());
    let v = config_cli::run(vec![
        "validate".into(),
        "--config".into(),
        path.display().to_string(),
    ])
    .unwrap();
    assert!(v.contains("unknown_top_level"));
    assert!(config_cli::run(vec![
        "validate".into(),
        "--write".into(),
        "--config".into(),
        path.display().to_string()
    ])
    .is_err());
}
#[cfg(unix)]
#[test]
fn refuses_symlink_writes() {
    use std::os::unix::fs::symlink;
    let fixture = FixtureDir::new();
    let target = fixture.path("real.toml");
    let link = fixture.path("link.toml");
    fs::write(&target, "panel_height = 48").unwrap();
    symlink(&target, &link).unwrap();
    let p = plan(&link, &fs::read_to_string(&link).unwrap()).unwrap();
    assert_eq!(write_explicit(&link, &p).unwrap_err().code, "unsafe_path");
    assert!(!fixture.path("link.toml.bak").exists());
}
