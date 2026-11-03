//! Explicit opt-in configuration migration CLI. Register as nuraloumi-config
//! in Cargo.toml through the shared integration lane, not this isolated lane.
use nuraloumi_shell::config_migration;

use config_migration::{plan, prepare_reload, read_bounded, write_explicit, WriteOutcome};
use std::path::PathBuf;

const HELP: &str = "nuraloumi-config validate --config PATH [--json]
nuraloumi-config migrate --config PATH [--dry-run | --write] [--json]

validate: read-only parse and semantic diagnostics for v0/v1 TOML or JSON
migrate:  v0 -> v1 proposal; defaults to dry-run, prints resulting config
--write:  explicit atomic replacement; creates PATH.bak without overwriting it
No startup rewriting, no implicit version upgrades, no runtime reload.";
fn fail(msg: impl Into<String>) -> String {
    msg.into()
}

pub fn run(args: impl IntoIterator<Item = String>) -> Result<String, String> {
    let mut args = args.into_iter();
    let command = args.next().unwrap_or_default();
    if command == "--help" || command == "-h" || command.is_empty() {
        return Ok(HELP.into());
    }
    if command != "validate" && command != "migrate" {
        return Err(fail(format!("unknown command {command:?}\n{HELP}")));
    }
    let mut path = None;
    let mut write = false;
    let mut dry_run = false;
    let mut json = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--config" if path.is_none() => {
                path = Some(PathBuf::from(args.next().ok_or("--config requires PATH")?));
            }
            "--write" if !write => write = true,
            "--dry-run" if !dry_run => dry_run = true,
            "--json" if !json => json = true,
            "--help" | "-h" => return Ok(HELP.into()),
            _ => return Err(fail(format!("unexpected or duplicate argument {arg:?}"))),
        }
    }
    if command == "validate" && (write || dry_run) {
        return Err(fail(
            "validate is read-only; --write/--dry-run belong to migrate",
        ));
    }
    if write && dry_run {
        return Err(fail("--write conflicts with --dry-run"));
    }
    let path = path.ok_or("--config PATH is required")?;
    let input = read_bounded(&path).map_err(|e| e.to_string())?;
    if command == "validate" {
        let report = prepare_reload(&path, &input).map_err(|e| e.to_string())?;
        if json {
            return serde_json::to_string_pretty(&serde_json::json!({
                "ok": true, "version": report.source_version,
                "diagnostics": report.diagnostics
            }))
            .map_err(|e| e.to_string());
        }
        let mut result = format!(
            "valid: {} (config_version={})",
            path.display(),
            report.source_version
        );
        for diagnostic in report.diagnostics {
            result.push_str(&format!(
                "\n{:?}: {}: {}: {}",
                diagnostic.severity, diagnostic.code, diagnostic.location, diagnostic.message
            ));
        }
        return Ok(result);
    }
    let proposal = plan(&path, &input).map_err(|e| e.to_string())?;
    let outcome = if write {
        Some(write_explicit(&path, &proposal).map_err(|e| e.to_string())?)
    } else {
        None
    };
    if json {
        return serde_json::to_string_pretty(&serde_json::json!({
            "ok": true, "from_version": proposal.from_version,
            "to_version": proposal.to_version, "changed": proposal.changed,
            "written": outcome == Some(WriteOutcome::Updated),
            "backup": if outcome == Some(WriteOutcome::Updated) {
                Some(format!("{}.bak", path.display()))
            } else { None },
            "diagnostics": proposal.diagnostics,
            "migrated_config": proposal.output,
        }))
        .map_err(|e| e.to_string());
    }
    if write {
        return Ok(match outcome {
            Some(WriteOutcome::Updated) => format!(
                "updated {} (v0 -> v1); backup: {}.bak",
                path.display(),
                path.display()
            ),
            _ => format!("unchanged: {} (already v1)", path.display()),
        });
    }
    Ok(format!(
        "# dry-run (no files changed): {} (v{} -> v{})\n{}",
        path.display(),
        proposal.from_version,
        proposal.to_version,
        proposal.output
    ))
}

pub fn main() {
    match run(std::env::args().skip(1)) {
        Ok(output) => println!("{output}"),
        Err(message) => {
            eprintln!("config_error: {message}");
            std::process::exit(2);
        }
    }
}
