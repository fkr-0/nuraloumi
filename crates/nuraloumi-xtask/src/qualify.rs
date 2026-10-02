use crate::package::{host_triple, release_dir, target_root};
use crate::process::{command_available, run_capture, run_capture_env, run_checked};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

pub const ARMV7_TARGET: &str = "armv7-unknown-linux-musleabihf";

pub fn qualify_host(root: &Path, strict: bool, nested: bool) -> Result<(), String> {
    println!("HOST_TRIPLE={}", host_triple(root)?);

    let mut pending = Vec::new();

    match env::var("NURALOUMI_FIXTURE_SMOKE") {
        Ok(command) if !command.trim().is_empty() => {
            run_shell_smoke(root, "FIXTURE_HEADLESS", &command)?;
            let fixture_pngs = nonempty_pngs(&root.join("tests/fixtures/render"))?;
            if fixture_pngs.is_empty() {
                println!(
                    "FIXTURE_PNG=PENDING reason=custom-smoke-produced-no-repository-fixture-png"
                );
                pending.push("fixture-png");
            } else {
                println!("FIXTURE_PNG=PASS count={}", fixture_pngs.len());
            }
        }
        _ if root
            .join("crates/nuraloumi-render-cairo/examples/render_fixtures.rs")
            .is_file() =>
        {
            let fixture_dir = target_root(root)
                .join("nuraloumi-qualification")
                .join("render-fixtures");
            if fixture_dir.exists() {
                fs::remove_dir_all(&fixture_dir)
                    .map_err(|error| format!("clear {}: {error}", fixture_dir.display()))?;
            }
            fs::create_dir_all(&fixture_dir)
                .map_err(|error| format!("create {}: {error}", fixture_dir.display()))?;
            let fixture_dir_text = fixture_dir.to_string_lossy();
            let output = run_capture(
                "cargo",
                &[
                    "run",
                    "-q",
                    "--locked",
                    "-p",
                    "nuraloumi-render-cairo",
                    "--example",
                    "render_fixtures",
                    "--",
                    fixture_dir_text.as_ref(),
                ],
                root,
            )?;
            if !output.status.success() {
                return Err(format!(
                    "renderer fixture smoke failed with {}\nstdout:\n{}\nstderr:\n{}",
                    output.status,
                    output.stdout.trim_end(),
                    output.stderr.trim_end()
                ));
            }
            println!("FIXTURE_HEADLESS=PASS renderer-example");
            let fixture_pngs = nonempty_pngs(&fixture_dir)?;
            if fixture_pngs.is_empty() {
                return Err(format!(
                    "renderer fixture smoke completed but produced no non-empty PNG in {}",
                    fixture_dir.display()
                ));
            }
            println!(
                "FIXTURE_PNG=PASS count={} directory={}",
                fixture_pngs.len(),
                fixture_dir.display()
            );
        }
        _ => {
            let fixture_pngs = nonempty_pngs(&root.join("tests/fixtures/render"))?;
            if fixture_pngs.is_empty() {
                println!("FIXTURE_PNG=PENDING reason=no-renderer-example-or-nonempty-fixture-png");
                pending.push("fixture-png");
            } else {
                println!("FIXTURE_PNG=PASS count={}", fixture_pngs.len());
            }
            println!(
                "FIXTURE_HEADLESS=PENDING reason=set-NURALOUMI_FIXTURE_SMOKE-after-renderer-lane-lands"
            );
            pending.push("fixture-headless");
        }
    }

    match env::var("NURALOUMI_MENU_SMOKE") {
        Ok(command) if !command.trim().is_empty() => {
            run_shell_smoke(root, "MENU_HEADLESS", &command)?;
        }
        _ => {
            let fixture = root.join("examples/menu-fixtures/launcher.json");
            match (find_host_binary(root, "nuraloumi-menu"), fixture.is_file()) {
                (Some(menu), true) => {
                    let menu_text = menu.to_string_lossy();
                    let fixture_text = fixture.to_string_lossy();
                    let output = run_capture(
                        menu_text.as_ref(),
                        &[
                            "--headless",
                            "--fixture",
                            fixture_text.as_ref(),
                            "--input",
                            "down,enter",
                        ],
                        root,
                    )?;
                    if output.status.success()
                        && output.stdout.contains("\"mode\": \"headless\"")
                        && output.stdout.contains("\"reports\"")
                    {
                        println!(
                            "MENU_HEADLESS=PASS fixture={} binary={}",
                            fixture.display(),
                            menu.display()
                        );
                    } else {
                        return Err(format!(
                            "menu headless fixture smoke failed or returned unexpected output\nstdout:\n{}\nstderr:\n{}",
                            output.stdout.trim_end(),
                            output.stderr.trim_end()
                        ));
                    }
                }
                (None, _) => {
                    println!("MENU_HEADLESS=PENDING reason=nuraloumi-menu-not-built");
                    pending.push("menu-headless");
                }
                (Some(_), false) => {
                    println!("MENU_HEADLESS=PENDING reason=launcher-fixture-not-landed");
                    pending.push("menu-headless");
                }
            }
        }
    }

    if nested {
        if command_available("weston", root) {
            let script = root.join("scripts/smoke-host.sh");
            let output = run_capture(
                "sh",
                &[script.to_string_lossy().as_ref(), "--nested-only"],
                root,
            )?;
            print!("{}", output.stdout);
            eprint!("{}", output.stderr);
            if !output.status.success() || !output.stdout.contains("NESTED_PIXMAN=PASS") {
                return Err(format!(
                    "nested compositor smoke did not pass with status {}",
                    output.status
                ));
            }
        } else {
            println!("NESTED_COMPOSITOR=SKIP reason=weston-not-installed");
        }
    }

    if pending.is_empty() {
        println!("HOST_QUALIFICATION=PASS");
        Ok(())
    } else {
        println!("HOST_QUALIFICATION=PENDING pending={}", pending.join(","));
        if strict {
            Err(format!(
                "host qualification pending: {}",
                pending.join(", ")
            ))
        } else {
            Ok(())
        }
    }
}

pub fn qualify_armv7(root: &Path, cross_check: bool) -> Result<(), String> {
    println!("TARGET={ARMV7_TARGET}");
    println!("TARGET_POLICY=musl-armv7-hardfloat-no-neon");
    println!("DEVICE_RUNTIME=PENDING reason=requires-real-SL101-run");

    let config = root.join(".cargo/config.toml");
    let config_text = fs::read_to_string(&config)
        .map_err(|error| format!("read {}: {error}", config.display()))?;
    if config_text.contains("[target.armv7-unknown-linux-musleabihf]")
        && config_text.contains("target-feature=-neon,-d32,-hwdiv,-hwdiv-arm")
    {
        println!("NO_NEON_CONFIG=PASS");
        println!("TEGRA20_CODEGEN_CONFIG=PASS vfp=VFPv3-D16 d32=off hwdiv=off");
    } else {
        return Err(format!(
            "{} must explicitly disable NEON, d32, hwdiv and hwdiv-arm for {}",
            config.display(),
            ARMV7_TARGET
        ));
    }

    let target_installed = installed_rust_targets(root)?
        .iter()
        .any(|target| target == ARMV7_TARGET);
    if target_installed {
        println!("RUST_TARGET=PASS installed");
    } else {
        println!("RUST_TARGET=PENDING reason=rust-target-not-installed");
    }

    let available_linkers = arm_musl_linkers(root)?;
    if available_linkers.is_empty() {
        println!("CROSS_LINKER=PENDING reason=no-verified-arm-musl-linker");
    } else {
        println!(
            "CROSS_LINKER=PASS candidates={}",
            available_linkers.join(",")
        );
    }

    if cross_check {
        if !target_installed {
            println!("CROSS_CHECK=PENDING reason=target-not-installed");
        } else {
            let cross_root = env::var_os("NURALOUMI_SL101_CROSS_ROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("target/sl101-cross"));
            let sysroot = env::var_os("NURALOUMI_SL101_SYSROOT")
                .map(PathBuf::from)
                .unwrap_or_else(|| cross_root.join("sysroot"));
            let pkgconfig = env::var_os("NURALOUMI_SL101_PKGCONFIG")
                .map(PathBuf::from)
                .unwrap_or_else(|| cross_root.join("pkgconfig"));
            let linker = root.join("scripts/armv7-sl101-linker.sh");
            if !sysroot.join("lib/ld-musl-armhf.so.1").is_file()
                || !pkgconfig.join("cairo.pc").is_file()
                || !linker.is_file()
            {
                println!(
                    "CROSS_CHECK=PENDING reason=sl101-sysroot-not-prepared hint=scripts/prepare-sl101-sysroot.sh"
                );
            } else {
                let sysroot_text = sysroot.to_string_lossy().into_owned();
                let pkgconfig_text = pkgconfig.to_string_lossy().into_owned();
                let linker_text = linker.to_string_lossy().into_owned();
                let envs = [
                    (
                        "CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER",
                        linker_text.as_str(),
                    ),
                    ("NURALOUMI_SL101_SYSROOT", sysroot_text.as_str()),
                    ("PKG_CONFIG_ALLOW_CROSS", "1"),
                    ("PKG_CONFIG_SYSROOT_DIR", sysroot_text.as_str()),
                    ("PKG_CONFIG_LIBDIR", pkgconfig_text.as_str()),
                    ("PKG_CONFIG_PATH", pkgconfig_text.as_str()),
                ];
                let output = run_capture_env(
                    "cargo",
                    &["check", "--locked", "--workspace", "--target", ARMV7_TARGET],
                    root,
                    &envs,
                )?;
                if output.status.success() {
                    println!("CROSS_CHECK=PASS compiler-only-not-device-proof");
                } else {
                    return Err(format!(
                        "cross cargo check failed\nstdout:\n{}\nstderr:\n{}",
                        output.stdout.trim_end(),
                        output.stderr.trim_end()
                    ));
                }
            }
        }
    } else {
        println!("CROSS_CHECK=PENDING reason=not-requested-use---cross-check");
    }

    let release = target_root(root).join(ARMV7_TARGET).join("release");
    let binaries = target_binaries(&release)?;
    if binaries.is_empty() {
        println!(
            "ELF_AUDIT=PENDING reason=no-cross-release-binaries path={}",
            release.display()
        );
    } else {
        let script = root.join("scripts/inspect-armv7-elf.sh");
        let mut audited = 0usize;
        let mut audit_pending = 0usize;
        for binary in binaries {
            let output = run_capture(
                "sh",
                &[
                    script.to_string_lossy().as_ref(),
                    binary.to_string_lossy().as_ref(),
                ],
                root,
            )?;
            print!("{}", output.stdout);
            eprint!("{}", output.stderr);
            if output.status.success() {
                audited += 1;
            } else if output.status.code() == Some(2) {
                audit_pending += 1;
            } else {
                return Err(format!(
                    "ARMv7 ELF audit failed for {} with {}",
                    binary.display(),
                    output.status
                ));
            }
        }
        if audit_pending == 0 {
            println!("ELF_AUDIT=PASS binaries={audited} heuristic-only");
        } else {
            println!(
                "ELF_AUDIT=PENDING passed={} pending={} heuristic-incomplete",
                audited, audit_pending
            );
        }
    }

    println!(
        "ARMV7_QUALIFICATION=PENDING reason=compiler-and-ELF-proof-do-not-replace-device-runtime"
    );
    Ok(())
}

fn arm_musl_linkers(root: &Path) -> Result<Vec<String>, String> {
    let mut candidates = vec![
        root.join("scripts/armv7-sl101-linker.sh")
            .to_string_lossy()
            .into_owned(),
        "armv7-unknown-linux-musleabihf-gcc".to_owned(),
        "arm-linux-musleabihf-gcc".to_owned(),
        "armv7-alpine-linux-musleabihf-gcc".to_owned(),
    ];
    if let Ok(linker) = env::var("CARGO_TARGET_ARMV7_UNKNOWN_LINUX_MUSLEABIHF_LINKER") {
        if !linker.trim().is_empty() {
            candidates.push(linker);
        }
    }
    candidates.sort();
    candidates.dedup();

    let mut verified = Vec::new();
    for linker in candidates {
        if !command_available(&linker, root) && !Path::new(&linker).is_file() {
            continue;
        }
        let output = run_capture(&linker, &["-dumpmachine"], root)?;
        if !output.status.success() {
            println!(
                "CROSS_LINKER_REJECTED candidate={} reason=dumpmachine-failed",
                linker
            );
            continue;
        }
        let triple = output.stdout.trim().to_ascii_lowercase();
        if triple.contains("arm") && triple.contains("musl") {
            verified.push(format!("{linker}[{triple}]"));
        } else {
            println!(
                "CROSS_LINKER_REJECTED candidate={} triple={}",
                linker,
                if triple.is_empty() {
                    "<empty>"
                } else {
                    &triple
                }
            );
        }
    }
    Ok(verified)
}

fn installed_rust_targets(root: &Path) -> Result<Vec<String>, String> {
    if !command_available("rustup", root) {
        return Ok(Vec::new());
    }
    let output = run_checked("rustup", &["target", "list", "--installed"], root)?;
    Ok(output
        .stdout
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect())
}

fn find_host_binary(root: &Path, name: &str) -> Option<PathBuf> {
    let debug = target_root(root).join("debug").join(name);
    if debug.is_file() {
        return Some(debug);
    }
    let release = release_dir(root, None).join(name);
    if release.is_file() {
        return Some(release);
    }
    None
}

fn nonempty_pngs(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    visit_pngs(dir, &mut result)?;
    result.sort();
    Ok(result)
}

fn visit_pngs(dir: &Path, result: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|error| format!("read {}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("read {} entry: {error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            visit_pngs(&path, result)?;
        } else if path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("png"))
            && fs::metadata(&path)
                .map_err(|error| format!("stat {}: {error}", path.display()))?
                .len()
                > 0
        {
            result.push(path);
        }
    }
    Ok(())
}

fn run_shell_smoke(root: &Path, label: &str, command: &str) -> Result<(), String> {
    let output = run_capture("sh", &["-c", command], root)?;
    if output.status.success() {
        println!("{label}=PASS");
        if !output.stdout.trim().is_empty() {
            println!("{}", output.stdout.trim_end());
        }
        Ok(())
    } else {
        Err(format!(
            "{label} command failed with {}\nstdout:\n{}\nstderr:\n{}",
            output.status,
            output.stdout.trim_end(),
            output.stderr.trim_end()
        ))
    }
}

fn target_binaries(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut binaries = Vec::new();
    for name in ["nuraloumi-panel", "nuraloumi-menu", "nuraloumi-probe"] {
        let path = dir.join(name);
        if path.is_file() {
            binaries.push(path);
        }
    }
    Ok(binaries)
}

#[cfg(test)]
mod tests {
    use super::nonempty_pngs;
    use std::fs;

    #[test]
    fn png_probe_ignores_empty_files() {
        let dir = std::env::temp_dir().join(format!("nuraloumi-xtask-pngs-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create");
        fs::write(dir.join("empty.png"), []).expect("empty");
        fs::write(dir.join("real.PNG"), [1_u8, 2, 3]).expect("real");
        let pngs = nonempty_pngs(&dir).expect("probe");
        assert_eq!(pngs.len(), 1);
        assert_eq!(
            pngs[0].file_name().and_then(|name| name.to_str()),
            Some("real.PNG")
        );
        fs::remove_dir_all(dir).expect("cleanup");
    }
}
