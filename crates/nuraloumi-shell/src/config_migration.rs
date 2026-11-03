//! Deterministic, opt-in migration of *supported* NuraLoumi shell config versions.
//! No startup integration and no runtime command execution. The accepted migration
//! is the real legacy v0 (missing/zero version) to current v1 schema.
use crate::ShellConfig;
use serde::Serialize;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const CURRENT_VERSION: u32 = 1;
const MAX_CONFIG_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Diagnostic {
    pub severity: Severity,
    pub code: &'static str,
    pub location: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Toml,
    Json,
}

#[derive(Debug, Clone)]
pub struct ValidatedReload {
    pub config: ShellConfig,
    pub diagnostics: Vec<Diagnostic>,
    pub source_version: u32,
}

/// A pure, validated proposal. No filesystem access is performed by `plan`.
#[derive(Debug, Clone)]
pub struct MigrationPlan {
    pub from_version: u32,
    pub to_version: u32,
    pub changed: bool,
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
    original: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigrationError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for MigrationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}
impl std::error::Error for MigrationError {}

fn error(code: &'static str, message: impl Into<String>) -> MigrationError {
    MigrationError {
        code,
        message: message.into(),
    }
}

fn format_from_path(path: &Path) -> Result<Format, MigrationError> {
    match path.extension().and_then(|x| x.to_str()) {
        Some("toml") => Ok(Format::Toml),
        Some("json") => Ok(Format::Json),
        _ => Err(error(
            "unsupported_format",
            format!("{}: expected .toml or .json", path.display()),
        )),
    }
}

fn ensure_bounded(text: &str) -> Result<(), MigrationError> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err(error("config_too_large", "configuration exceeds 1 MiB"));
    }
    Ok(())
}

fn parse_typed(format: Format, text: &str) -> Result<ShellConfig, MigrationError> {
    let config: ShellConfig = match format {
        Format::Toml => toml::from_str(text).map_err(|e| error("parse_error", e.to_string()))?,
        Format::Json => {
            serde_json::from_str(text).map_err(|e| error("parse_error", e.to_string()))?
        }
    };
    config
        .validate()
        .map_err(|e| error("validation_error", e))?;
    Ok(config)
}

fn version_from_toml(value: &toml::Value) -> Result<u32, MigrationError> {
    let table = value
        .as_table()
        .ok_or_else(|| error("invalid_root", "TOML root must be a table"))?;
    match table.get("config_version") {
        None => Ok(0),
        Some(value) => value
            .as_integer()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                error(
                    "invalid_version",
                    "config_version must be a nonnegative 32-bit integer",
                )
            }),
    }
}

fn version_from_json(value: &serde_json::Value) -> Result<u32, MigrationError> {
    let object = value
        .as_object()
        .ok_or_else(|| error("invalid_root", "JSON root must be an object"))?;
    match object.get("config_version") {
        None => Ok(0),
        Some(value) => value
            .as_u64()
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                error(
                    "invalid_version",
                    "config_version must be a nonnegative 32-bit integer",
                )
            }),
    }
}

fn ensure_supported_version(version: u32) -> Result<(), MigrationError> {
    if version > CURRENT_VERSION {
        return Err(error(
            "unsupported_version",
            format!("config_version {version} is newer than supported version {CURRENT_VERSION}"),
        ));
    }
    Ok(())
}

fn diagnostics(config: &ShellConfig, source_version: u32) -> Vec<Diagnostic> {
    let mut messages = Vec::new();
    if source_version == 0 {
        messages.push(Diagnostic {
            severity: Severity::Info,
            code: "legacy_version",
            location: "config_version".into(),
            message: "legacy v0 will be upgraded to v1 only on explicit write".into(),
        });
    }
    for key in config.extra.keys() {
        messages.push(Diagnostic {
            severity: Severity::Warning,
            code: "unknown_top_level",
            location: key.clone(),
            message: "extension retained; ignored by the shell".into(),
        });
    }
    messages
}

/// Validate a file candidate for live reload without applying it. Caller must
/// atomically swap the complete validated configuration at a runtime safe point.
/// This does *not* advertise that IPC reload is already wired.
pub fn prepare_reload(path: &Path, source: &str) -> Result<ValidatedReload, MigrationError> {
    ensure_bounded(source)?;
    let format = format_from_path(path)?;
    let (version, config) = match format {
        Format::Toml => {
            let raw: toml::Value =
                toml::from_str(source).map_err(|e| error("parse_error", e.to_string()))?;
            let version = version_from_toml(&raw)?;
            ensure_supported_version(version)?;
            (version, parse_typed(format, source)?)
        }
        Format::Json => {
            let raw: serde_json::Value =
                serde_json::from_str(source).map_err(|e| error("parse_error", e.to_string()))?;
            let version = version_from_json(&raw)?;
            ensure_supported_version(version)?;
            (version, parse_typed(format, source)?)
        }
    };
    Ok(ValidatedReload {
        diagnostics: diagnostics(&config, version),
        config,
        source_version: version,
    })
}

/// Plan v0 -> v1. Re-validate the transformed payload and check semantic
/// equivalence of every existing typed section, including flattened extras.
/// v1 input remains byte-for-byte unchanged (no gratuitous formatting churn).
pub fn plan(path: &Path, source: &str) -> Result<MigrationPlan, MigrationError> {
    let before = prepare_reload(path, source)?;
    if before.source_version == CURRENT_VERSION {
        return Ok(MigrationPlan {
            from_version: CURRENT_VERSION,
            to_version: CURRENT_VERSION,
            changed: false,
            output: source.into(),
            diagnostics: before.diagnostics,
            original: source.into(),
        });
    }
    let format = format_from_path(path)?;
    let mut output = match format {
        Format::Toml => {
            let mut value: toml::Value =
                toml::from_str(source).map_err(|e| error("parse_error", e.to_string()))?;
            value
                .as_table_mut()
                .ok_or_else(|| error("invalid_root", "expected TOML table"))?
                .insert(
                    "config_version".into(),
                    toml::Value::Integer(i64::from(CURRENT_VERSION)),
                );
            toml::to_string_pretty(&value).map_err(|e| error("serialize_error", e.to_string()))?
        }
        Format::Json => {
            let mut value: serde_json::Value =
                serde_json::from_str(source).map_err(|e| error("parse_error", e.to_string()))?;
            value
                .as_object_mut()
                .ok_or_else(|| error("invalid_root", "expected JSON object"))?
                .insert("config_version".into(), serde_json::json!(CURRENT_VERSION));
            serde_json::to_string_pretty(&value)
                .map_err(|e| error("serialize_error", e.to_string()))?
        }
    };
    if !output.ends_with('\n') {
        output.push('\n');
    }
    let after = prepare_reload(path, &output)?;
    let mut expected = before.config;
    expected.config_version = CURRENT_VERSION;
    if after.config != expected {
        return Err(error(
            "roundtrip_mismatch",
            "migration would change existing configuration semantics; refusing write",
        ));
    }
    Ok(MigrationPlan {
        from_version: 0,
        to_version: CURRENT_VERSION,
        changed: true,
        output,
        diagnostics: before.diagnostics,
        original: source.into(),
    })
}

/// Read only bounded input; config is never changed by discovery/startup.
pub fn read_bounded(path: &Path) -> Result<String, MigrationError> {
    let file =
        File::open(path).map_err(|e| error("io_error", format!("{}: {e}", path.display())))?;
    let mut input = String::new();
    file.take((MAX_CONFIG_BYTES + 1) as u64)
        .read_to_string(&mut input)
        .map_err(|e| error("io_error", format!("{}: {e}", path.display())))?;
    ensure_bounded(&input)?;
    Ok(input)
}

fn ensure_regular_file(path: &Path) -> Result<fs::Metadata, MigrationError> {
    let meta = fs::symlink_metadata(path)
        .map_err(|e| error("io_error", format!("{}: {e}", path.display())))?;
    if !meta.file_type().is_file() {
        return Err(error(
            "unsafe_path",
            format!(
                "{} must be a regular file, not a symlink or directory",
                path.display()
            ),
        ));
    }
    Ok(meta)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteOutcome {
    Unchanged,
    Updated,
}

struct RemoveOnDrop(PathBuf);
impl Drop for RemoveOnDrop {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Explicit replacement only: exclusive backup, same-directory exclusive temp,
/// fsync and atomic rename. Fail closed when the on-disk source has changed.
/// Never removes or overwrites a previous backup. No implicit startup writes.
pub fn write_explicit(
    path: &Path,
    migration: &MigrationPlan,
) -> Result<WriteOutcome, MigrationError> {
    if !migration.changed {
        return Ok(WriteOutcome::Unchanged);
    }
    let metadata = ensure_regular_file(path)?;
    if read_bounded(path)? != migration.original {
        return Err(error(
            "concurrent_change",
            "source changed after dry-run planning",
        ));
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let filename = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| error("unsafe_path", "source filename is not valid UTF-8"))?;
    let backup = parent.join(format!("{filename}.bak"));

    // An existing backup is a deliberate stop, not an invitation to overwrite.
    let mut backup_opts = OpenOptions::new();
    backup_opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        backup_opts.mode(0o600);
    }
    let mut backup_file = backup_opts
        .open(&backup)
        .map_err(|e| error("backup_error", format!("{}: {e}", backup.display())))?;
    backup_file
        .write_all(migration.original.as_bytes())
        .and_then(|_| backup_file.sync_all())
        .map_err(|e| error("backup_error", format!("{}: {e}", backup.display())))?;
    drop(backup_file);

    let mut temporary = None;
    for attempt in 0..16 {
        let tmp = parent.join(format!(
            ".{filename}.nuraloumi-{}-{attempt}.tmp",
            std::process::id()
        ));
        let mut opts = OpenOptions::new();
        opts.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        match opts.open(&tmp) {
            Ok(f) => {
                temporary = Some((tmp, f));
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(error("io_error", format!("{}: {e}", tmp.display()))),
        }
    }
    let (tmp, mut file) =
        temporary.ok_or_else(|| error("io_error", "no free temporary filename"))?;
    let cleanup = RemoveOnDrop(tmp.clone());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(
            metadata.permissions().mode() & 0o777,
        ))
        .map_err(|e| error("io_error", e.to_string()))?;
    }
    file.write_all(migration.output.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| error("io_error", format!("temporary config: {e}")))?;
    drop(file);

    ensure_regular_file(path)?;
    if read_bounded(path)? != migration.original {
        return Err(error(
            "concurrent_change",
            "source changed before atomic replacement",
        ));
    }
    fs::rename(&tmp, path).map_err(|e| error("io_error", format!("atomic rename: {e}")))?;
    drop(cleanup);
    File::open(parent)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| error("io_error", format!("sync config directory: {e}")))?;
    Ok(WriteOutcome::Updated)
}
