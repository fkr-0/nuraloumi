#![forbid(unsafe_code)]

mod package;
mod process;
mod qualify;

use package::{print_sizes, stage_package};
use process::run_capture;
use qualify::{qualify_armv7, qualify_host, ARMV7_TARGET};
use std::env;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const RUNTIME_PACKAGES: &[&str] = &[
    "nuraloumi-core",
    "nuraloumi-render-cairo",
    "nuraloumi-wayland",
    "nuraloumi-providers",
    "nuraloumi-shell",
];

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let root = repo_root()?;
    let args = env::args().skip(1).collect::<Vec<_>>();
    let Some(command) = args.first().map(String::as_str) else {
        print_help();
        return Ok(());
    };

    match command {
        "-h" | "--help" | "help" => {
            print_help();
            Ok(())
        }
        "check" => cmd_check(&root, &args[1..]),
        "build-release" => cmd_build_release(&root, &args[1..]),
        "size" => cmd_size(&root, &args[1..]),
        "deps" => cmd_deps(&root, &args[1..]),
        "package" => cmd_package(&root, &args[1..]),
        "qualify-host" => cmd_qualify_host(&root, &args[1..]),
        "qualify-armv7" => cmd_qualify_armv7(&root, &args[1..]),
        other => Err(format!("unknown subcommand {other:?}; use --help")),
    }
}

fn repo_root() -> Result<PathBuf, String> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot determine repository root from CARGO_MANIFEST_DIR".to_owned())
}

fn cmd_check(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: nuraloumi-xtask check [--xtask-only]");
        return Ok(());
    }
    reject_unknown(args, &["--xtask-only"])?;
    let xtask_only = args.iter().any(|arg| arg == "--xtask-only");

    let steps: Vec<(&str, Vec<&str>)> = if xtask_only {
        vec![
            (
                "fmt",
                vec!["fmt", "--package", "nuraloumi-xtask", "--", "--check"],
            ),
            (
                "check",
                vec!["check", "--package", "nuraloumi-xtask", "--all-targets"],
            ),
            ("test", vec!["test", "--package", "nuraloumi-xtask"]),
            (
                "clippy",
                vec![
                    "clippy",
                    "--package",
                    "nuraloumi-xtask",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            ),
        ]
    } else {
        vec![
            ("fmt", vec!["fmt", "--all", "--", "--check"]),
            ("check", vec!["check", "--workspace", "--all-targets"]),
            ("test", vec!["test", "--workspace"]),
            (
                "clippy",
                vec![
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            ),
        ]
    };

    for (name, step_args) in steps {
        println!("==> {name}: cargo {}", step_args.join(" "));
        let output = run_capture("cargo", &step_args, root)?;
        if !output.stdout.trim().is_empty() {
            print!("{}", output.stdout);
        }
        if !output.stderr.trim().is_empty() {
            eprint!("{}", output.stderr);
        }
        if !output.status.success() {
            return Err(format!("{name} failed with {}", output.status));
        }
    }
    println!(
        "CHECK=PASS scope={}",
        if xtask_only { "xtask" } else { "workspace" }
    );
    Ok(())
}

fn cmd_build_release(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: nuraloumi-xtask build-release [--target TRIPLE]");
        return Ok(());
    }
    let target = option_value(args, "--target")?;
    let mut cargo_args = vec!["build", "--release", "--workspace", "--bins"];
    if let Some(target) = target.as_deref() {
        cargo_args.push("--target");
        cargo_args.push(target);
    }
    reject_consumed_options(args, &["--target"])?;

    let output = run_capture("cargo", &cargo_args, root)?;
    print!("{}", output.stdout);
    eprint!("{}", output.stderr);
    if !output.status.success() {
        return Err(format!("release build failed with {}", output.status));
    }
    println!(
        "BUILD_RELEASE=PASS target={}",
        target.unwrap_or_else(|| "host".to_owned())
    );
    Ok(())
}

fn cmd_size(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: nuraloumi-xtask size [--target TRIPLE] [--sections]");
        return Ok(());
    }
    let target = option_value(args, "--target")?;
    let sections = args.iter().any(|arg| arg == "--sections");
    reject_consumed_options(args, &["--target", "--sections"])?;
    print_sizes(root, target.as_deref(), sections)
}

fn cmd_deps(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: nuraloumi-xtask deps [--strict]");
        return Ok(());
    }
    reject_unknown(args, &["--strict"])?;
    let strict = args.iter().any(|arg| arg == "--strict");
    let banned = [
        "x11", "xcb", "iced", "wgpu", "gtk", "qt", "slint", "egui", "eframe", "tauri",
    ];
    let mut hits = Vec::new();

    for package in RUNTIME_PACKAGES {
        let output = run_capture(
            "cargo",
            &["tree", "-p", package, "--edges", "normal,build"],
            root,
        )?;
        if !output.status.success() {
            return Err(format!(
                "cargo tree failed for {package}\nstdout:\n{}\nstderr:\n{}",
                output.stdout.trim_end(),
                output.stderr.trim_end()
            ));
        }

        println!("--- {package} ---");
        print!("{}", output.stdout);
        for line in output.stdout.lines() {
            let lower = line.to_ascii_lowercase();
            for needle in banned {
                if lower.contains(needle) {
                    hits.push(format!("{package}: {line}"));
                }
            }
        }
    }

    if hits.is_empty() {
        println!("DEPS_AUDIT=PASS no-prohibited-heavy-runtime-deps-detected");
        return Ok(());
    }

    println!("DEPS_AUDIT=WARNING hits={}", hits.len());
    for hit in &hits {
        println!("  {hit}");
    }
    if strict {
        Err("prohibited/heavy dependency candidates detected".to_owned())
    } else {
        Ok(())
    }
}

fn cmd_package(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: nuraloumi-xtask package [--target TRIPLE] [--out PATH]");
        return Ok(());
    }
    let target = option_value(args, "--target")?;
    let out = option_value(args, "--out")?.map(PathBuf::from);
    reject_consumed_options(args, &["--target", "--out"])?;
    stage_package(root, target.as_deref(), out.as_deref())?;
    Ok(())
}

fn cmd_qualify_host(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: nuraloumi-xtask qualify-host [--strict] [--no-nested]");
        println!("Environment:");
        println!("  NURALOUMI_FIXTURE_SMOKE  exact non-mutating renderer fixture command");
        println!("  NURALOUMI_MENU_SMOKE     exact non-mutating menu headless command");
        return Ok(());
    }
    reject_unknown(args, &["--strict", "--no-nested"])?;
    let strict = args.iter().any(|arg| arg == "--strict");
    let nested = !args.iter().any(|arg| arg == "--no-nested");
    qualify_host(root, strict, nested)
}

fn cmd_qualify_armv7(root: &Path, args: &[String]) -> Result<(), String> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        println!("Usage: nuraloumi-xtask qualify-armv7 [--cross-check]");
        println!("Target: {ARMV7_TARGET}");
        return Ok(());
    }
    reject_unknown(args, &["--cross-check"])?;
    let cross_check = args.iter().any(|arg| arg == "--cross-check");
    qualify_armv7(root, cross_check)
}

fn option_value(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut result = None;
    let mut index = 0usize;
    while index < args.len() {
        if args[index] == name {
            let value = args
                .get(index + 1)
                .ok_or_else(|| format!("{name} requires a value"))?;
            if value.starts_with('-') {
                return Err(format!("{name} requires a value, got {value:?}"));
            }
            if result.replace(value.clone()).is_some() {
                return Err(format!("{name} may only be specified once"));
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    Ok(result)
}

fn reject_unknown(args: &[String], allowed_flags: &[&str]) -> Result<(), String> {
    for arg in args {
        if !allowed_flags.contains(&arg.as_str()) {
            return Err(format!("unexpected argument {arg:?}"));
        }
    }
    Ok(())
}

fn reject_consumed_options(args: &[String], allowed: &[&str]) -> Result<(), String> {
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if !allowed.contains(&arg) {
            return Err(format!("unexpected argument {:?}", args[index]));
        }
        if arg == "--sections" {
            index += 1;
        } else {
            if index + 1 >= args.len() {
                return Err(format!("{arg} requires a value"));
            }
            index += 2;
        }
    }
    Ok(())
}

fn print_help() {
    println!(
        "NuraLoumi build / packaging / qualification tool

Usage:
  nuraloumi-xtask <command> [options]

Commands:
  check           Run fmt/check/test/clippy with explicit step boundaries
  build-release   Build workspace release binaries
  size            Enumerate release binary sizes; optionally show sections
  deps            Show runtime dependency trees and flag heavyweight GUI stacks
  package         Stage bin/share/config/license/readme plus deterministic manifest/tar
  qualify-host    Run non-mutating host qualification probes
  qualify-armv7   Report ARMv7 musl/no-NEON readiness without claiming device proof
  help            Show this help

Examples:
  cargo xtask check --xtask-only
  cargo xtask size --sections
  cargo xtask deps --strict
  cargo xtask qualify-host
  cargo xtask qualify-armv7 --cross-check"
    );
}

#[cfg(test)]
mod tests {
    use super::{option_value, reject_consumed_options, reject_unknown};

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn parses_single_option_value() {
        let args = strings(&["--target", "armv7-unknown-linux-musleabihf"]);
        assert_eq!(
            option_value(&args, "--target").expect("parse"),
            Some("armv7-unknown-linux-musleabihf".to_owned())
        );
    }

    #[test]
    fn rejects_duplicate_option() {
        let args = strings(&["--target", "a", "--target", "b"]);
        assert!(option_value(&args, "--target").is_err());
    }

    #[test]
    fn rejects_unknown_flag() {
        let args = strings(&["--wat"]);
        assert!(reject_unknown(&args, &["--strict"]).is_err());
    }

    #[test]
    fn consumed_options_accept_flag_and_value_pairs() {
        let args = strings(&["--target", "triple", "--sections"]);
        assert!(reject_consumed_options(&args, &["--target", "--sections"]).is_ok());
    }
}
