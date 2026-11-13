use crate::process::{command_available, run_checked};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

// The script uses Python's standard JSON parser with a bounded cargo-metadata
// invocation. No runtime crate or shared Cargo.lock modification is required.
fn runtime_binaries_from_list(list: &str) -> Result<Vec<String>, String> {
    let mut names = std::collections::BTreeSet::new();
    for line in list.lines() {
        let name = line.trim();
        if !name.starts_with("nuraloumi-")
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(format!("invalid runtime binary name: {name:?}"));
        }
        if !names.insert(name.to_owned()) {
            return Err(format!("duplicate runtime binary name: {name}"));
        }
    }
    if ![
        "nuraloumi-panel",
        "nuraloumi-menu",
        "nuraloumi-probe",
        "nuraloumi-thumbnail-helper",
    ]
    .iter()
    .all(|name| names.contains(*name))
    {
        return Err("Cargo metadata is missing a baseline runtime binary".to_owned());
    }
    Ok(names.into_iter().collect())
}

pub(crate) fn runtime_binaries(root: &Path) -> Result<Vec<String>, String> {
    let script = root.join("scripts/package-binaries.py");
    let output = run_checked("python3", &[script.to_string_lossy().as_ref()], root)?;
    runtime_binaries_from_list(&output.stdout)
}

pub fn host_triple(root: &Path) -> Result<String, String> {
    let output = run_checked("rustc", &["-vV"], root)?;
    output
        .stdout
        .lines()
        .find_map(|line| line.strip_prefix("host: ").map(str::to_owned))
        .ok_or_else(|| "rustc -vV did not report a host triple".to_owned())
}

pub fn git_commit(root: &Path) -> Result<String, String> {
    let output = run_checked("git", &["rev-parse", "HEAD"], root)?;
    Ok(output.stdout.trim().to_owned())
}

fn git_dirty(root: &Path) -> Result<bool, String> {
    let output = run_checked(
        "git",
        &["status", "--porcelain", "--untracked-files=normal"],
        root,
    )?;
    Ok(!output.stdout.trim().is_empty())
}

pub fn target_root(root: &Path) -> PathBuf {
    env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target"))
}

pub fn release_dir(root: &Path, target: Option<&str>) -> PathBuf {
    match target {
        Some(target) => target_root(root).join(target).join("release"),
        None => target_root(root).join("release"),
    }
}

pub fn executable_files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut files = Vec::new();
    for entry in fs::read_dir(dir).map_err(|error| format!("read {}: {error}", dir.display()))? {
        let entry = entry.map_err(|error| format!("read {} entry: {error}", dir.display()))?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata =
            fs::metadata(&path).map_err(|error| format!("stat {}: {error}", path.display()))?;
        if metadata.permissions().mode() & 0o111 != 0 {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

pub fn print_sizes(root: &Path, target: Option<&str>, sections: bool) -> Result<(), String> {
    let dir = release_dir(root, target);
    println!("release_dir={}", dir.display());

    let files = executable_files(&dir)?;
    if files.is_empty() {
        println!("PENDING no executable release binaries found");
        return Ok(());
    }

    for path in files {
        let size = fs::metadata(&path)
            .map_err(|error| format!("stat {}: {error}", path.display()))?
            .len();
        println!("{size:>10} {}", path.display());

        if sections {
            if command_available("size", root) {
                let output = run_checked("size", &["-A", path.to_string_lossy().as_ref()], root)?;
                print!("{}", output.stdout);
            } else if command_available("readelf", root) {
                let output =
                    run_checked("readelf", &["-S", path.to_string_lossy().as_ref()], root)?;
                print!("{}", output.stdout);
            } else {
                println!("PENDING section summary: neither size nor readelf is installed");
            }
        }
    }
    Ok(())
}

pub fn stage_package(
    root: &Path,
    target: Option<&str>,
    explicit_out: Option<&Path>,
) -> Result<PathBuf, String> {
    let resolved_target = match target {
        Some(target) => target.to_owned(),
        None => host_triple(root)?,
    };
    let version = workspace_version(root)?;
    let default_out = target_root(root)
        .join("nuraloumi-package")
        .join(&resolved_target)
        .join(format!("nuraloumi-{version}-{resolved_target}"));
    let stage = explicit_out.map(Path::to_path_buf).unwrap_or(default_out);
    let is_default = explicit_out.is_none();

    if stage.exists() {
        if is_default {
            fs::remove_dir_all(&stage)
                .map_err(|error| format!("clear {}: {error}", stage.display()))?;
        } else {
            return Err(format!(
                "refusing to overwrite explicit package directory {}; choose a new --out path",
                stage.display()
            ));
        }
    }

    let bin_dir = stage.join("bin");
    let share_dir = stage.join("share").join("nuraloumi");
    let config_dir = stage.join("config");
    fs::create_dir_all(&bin_dir)
        .map_err(|error| format!("create {}: {error}", bin_dir.display()))?;
    fs::create_dir_all(&share_dir)
        .map_err(|error| format!("create {}: {error}", share_dir.display()))?;
    fs::create_dir_all(&config_dir)
        .map_err(|error| format!("create {}: {error}", config_dir.display()))?;

    let binaries_dir = release_dir(root, target);
    let mut staged_bins = Vec::new();
    for name in runtime_binaries(root)? {
        let source = binaries_dir.join(&name);
        if !source.is_file() {
            return Err(format!(
                "required runtime release binary missing: {}; run the complete target release build first",
                source.display()
            ));
        }
        let destination = bin_dir.join(&name);
        fs::copy(&source, &destination).map_err(|error| {
            format!(
                "copy runtime binary {} -> {}: {error}",
                source.display(),
                destination.display()
            )
        })?;
        staged_bins.push(name);
    }

    copy_required(&root.join("packaging/README.md"), &stage.join("README.md"))?;
    copy_required(&root.join("packaging/LICENSE"), &stage.join("LICENSE"))?;
    copy_required(
        &root.join("packaging/nuraloumi.toml.example"),
        &config_dir.join("nuraloumi.toml.example"),
    )?;
    copy_required(
        &root.join("packaging/applications/nuraloumi-player.desktop"),
        &stage.join("share/applications/nuraloumi-player.desktop"),
    )?;
    copy_tree(
        &root.join("docs/qualification"),
        &share_dir.join("qualification"),
    )?;

    let commit = git_commit(root)?;
    let build_command = match target {
        Some(target) => format!("cargo build --release --workspace --bins --target {target}"),
        None => "cargo build --release --workspace --bins".to_owned(),
    };

    let mut files = collect_files(&stage)?;
    files.sort();
    let mut manifest = String::new();
    manifest.push_str("schema=nuraloumi-package-manifest-v1\n");
    manifest.push_str(&format!("version={version}\n"));
    manifest.push_str(&format!("target={resolved_target}\n"));
    manifest.push_str(&format!("git_commit={commit}\n"));
    manifest.push_str(&format!("git_dirty={}\n", git_dirty(root)?));
    manifest.push_str(&format!("build_command={build_command}\n"));
    manifest.push_str(&format!("binaries={}\n", staged_bins.join(",")));
    manifest.push_str("files:\n");

    for path in files {
        let relative = path
            .strip_prefix(&stage)
            .map_err(|error| format!("strip {}: {error}", path.display()))?;
        let digest = sha256(root, &path)?;
        manifest.push_str(&format!("  {digest}  {}\n", relative.display()));
    }
    fs::write(stage.join("MANIFEST.txt"), manifest)
        .map_err(|error| format!("write MANIFEST.txt: {error}"))?;

    let tar_path = archive_path(&stage);
    if tar_path.exists() {
        fs::remove_file(&tar_path)
            .map_err(|error| format!("remove {}: {error}", tar_path.display()))?;
    }
    create_deterministic_tar(root, &stage, &tar_path)?;
    println!("PACKAGE_STAGE={}", stage.display());
    println!("PACKAGE_TAR={}", tar_path.display());
    println!("PACKAGE_TAR_SHA256={}", sha256(root, &tar_path)?);
    Ok(stage)
}

pub fn check_package(root: &Path, stage: &Path) -> Result<(), String> {
    if !stage.is_dir() || stage.is_symlink() {
        return Err(format!(
            "package directory missing or symlinked: {}",
            stage.display()
        ));
    }
    let text = fs::read_to_string(stage.join("MANIFEST.txt"))
        .map_err(|error| format!("read package manifest: {error}"))?;
    let mut expected_files = std::collections::BTreeMap::new();
    let mut target = None;
    let mut declared_bins = None;
    let mut in_files = false;
    let mut keys = std::collections::BTreeSet::new();
    for line in text.lines() {
        if line == "files:" {
            if in_files {
                return Err("duplicate files marker".to_owned());
            }
            in_files = true;
            continue;
        }
        if in_files {
            let (hash, name) = line
                .trim_start()
                .split_once("  ")
                .ok_or_else(|| format!("invalid manifest file entry: {line:?}"))?;
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("invalid sha256 digest for {name:?}"));
            }
            let rel = Path::new(name);
            if rel
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
                || name.is_empty()
            {
                return Err(format!("unsafe package path: {name:?}"));
            }
            if expected_files
                .insert(name.to_owned(), hash.to_owned())
                .is_some()
            {
                return Err(format!("duplicate manifest path: {name}"));
            }
        } else {
            let (key, value) = line
                .split_once('=')
                .ok_or_else(|| format!("invalid manifest header: {line:?}"))?;
            if !keys.insert(key.to_owned()) {
                return Err(format!("duplicate manifest key: {key}"));
            }
            match key {
                "schema" if value == "nuraloumi-package-manifest-v1" => {}
                "target" => target = Some(value.to_owned()),
                "binaries" => {
                    declared_bins = Some(value.split(',').map(str::to_owned).collect::<Vec<_>>())
                }
                "version" | "git_commit" | "git_dirty" | "build_command" => {}
                _ => return Err(format!("invalid manifest key or schema: {key}={value}")),
            }
        }
    }
    if !in_files
        || ![
            "schema",
            "version",
            "target",
            "git_commit",
            "git_dirty",
            "build_command",
            "binaries",
        ]
        .iter()
        .all(|key| keys.contains(*key))
    {
        return Err("package manifest missing required fields".to_owned());
    }
    let bins = runtime_binaries(root)?;
    if declared_bins.as_ref() != Some(&bins) {
        return Err(format!("packaged binaries do not match Cargo targets: expected {bins:?}, found {declared_bins:?}"));
    }
    for required in [
        "README.md",
        "LICENSE",
        "config/nuraloumi.toml.example",
        "share/applications/nuraloumi-player.desktop",
        "share/nuraloumi/qualification/README.md",
    ] {
        if !expected_files.contains_key(required) {
            return Err(format!("required package input not listed: {required}"));
        }
    }
    for bin in &bins {
        let name = format!("bin/{bin}");
        if !expected_files.contains_key(&name) {
            return Err(format!("runtime binary missing from manifest: {name}"));
        }
        let file = stage.join(&name);
        let meta = fs::symlink_metadata(&file).map_err(|error| format!("stat {name}: {error}"))?;
        if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
            return Err(format!(
                "runtime binary is not a regular executable: {name}"
            ));
        }
        let mut header = [0_u8; 4];
        let mut input = fs::File::open(&file).map_err(|error| format!("open {name}: {error}"))?;
        std::io::Read::read_exact(&mut input, &mut header)
            .map_err(|error| format!("read ELF header {name}: {error}"))?;
        if &header != b"\x7fELF" {
            return Err(format!("runtime binary is not ELF: {name}"));
        }
    }
    let actual_files = collect_files(stage)?;
    if actual_files.len() != expected_files.len() {
        return Err(format!(
            "package file count mismatch: manifest={} actual={}",
            expected_files.len(),
            actual_files.len()
        ));
    }
    for file in actual_files {
        let name = file
            .strip_prefix(stage)
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned();
        let meta = fs::symlink_metadata(&file).map_err(|error| format!("stat {name}: {error}"))?;
        if !meta.is_file() {
            return Err(format!("package contains non-regular file: {name}"));
        }
        let expected = expected_files
            .get(&name)
            .ok_or_else(|| format!("unlisted package file: {name}"))?;
        let actual = sha256(root, &file)?;
        if &actual != expected {
            return Err(format!("package SHA256 mismatch for {name}"));
        }
    }
    if target.as_deref() == Some("armv7-unknown-linux-musleabihf") {
        let paths = bins
            .iter()
            .map(|bin| stage.join("bin").join(bin))
            .collect::<Vec<_>>();
        let args = paths
            .iter()
            .map(|path| {
                path.to_str()
                    .ok_or_else(|| "non-UTF8 binary path".to_owned())
            })
            .collect::<Result<Vec<_>, _>>()?;
        let script = root
            .join("scripts/inspect-armv7-elf.sh")
            .to_string_lossy()
            .into_owned();
        let mut audit_args = vec![script.as_str()];
        audit_args.extend(args);
        let audit = crate::process::run_capture("sh", &audit_args, root)?;
        print!("{}", audit.stdout);
        eprint!("{}", audit.stderr);
        if !audit.status.success() {
            return Err(format!(
                "ARMv7 package ELF audit incomplete or failed: {}",
                audit.status
            ));
        }
        println!("PACKAGE_ARMV7_ELF=PASS heuristic-only; SL101 runtime still PENDING");
    }
    println!(
        "PACKAGE_CHECK=PASS files={} binaries={} target={} hardware=PENDING",
        expected_files.len(),
        bins.len(),
        target.as_deref().unwrap_or("unknown")
    );
    Ok(())
}

fn archive_path(stage: &Path) -> PathBuf {
    let mut value = stage.as_os_str().to_os_string();
    value.push(".tar");
    PathBuf::from(value)
}

fn workspace_version(root: &Path) -> Result<String, String> {
    let cargo = fs::read_to_string(root.join("Cargo.toml"))
        .map_err(|error| format!("read Cargo.toml: {error}"))?;
    let mut in_workspace_package = false;
    for line in cargo.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_workspace_package = trimmed == "[workspace.package]";
            continue;
        }
        if in_workspace_package {
            if let Some(value) = trimmed.strip_prefix("version") {
                if let Some((_, value)) = value.split_once('=') {
                    return Ok(value.trim().trim_matches('"').to_owned());
                }
            }
        }
    }
    Err("workspace.package.version not found".to_owned())
}

fn copy_required(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_file() {
        return Err(format!(
            "required packaging input missing: {}",
            source.display()
        ));
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("create {}: {error}", parent.display()))?;
    }
    fs::copy(source, destination).map_err(|error| {
        format!(
            "copy {} -> {}: {error}",
            source.display(),
            destination.display()
        )
    })?;
    Ok(())
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    if !source.is_dir() {
        return Err(format!("required directory missing: {}", source.display()));
    }
    fs::create_dir_all(destination)
        .map_err(|error| format!("create {}: {error}", destination.display()))?;

    let mut entries = fs::read_dir(source)
        .map_err(|error| format!("read {}: {error}", source.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read {} entries: {error}", source.display()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let src = entry.path();
        let dst = destination.join(entry.file_name());
        if src.is_dir() {
            copy_tree(&src, &dst)?;
        } else if src.is_file() {
            fs::copy(&src, &dst)
                .map_err(|error| format!("copy {} -> {}: {error}", src.display(), dst.display()))?;
        }
    }
    Ok(())
}

fn collect_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut result = Vec::new();
    collect_files_inner(root, &mut result)?;
    Ok(result)
}

fn collect_files_inner(dir: &Path, result: &mut Vec<PathBuf>) -> Result<(), String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|error| format!("read {}: {error}", dir.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| format!("read {} entries: {error}", dir.display()))?;
    entries.sort_by_key(std::fs::DirEntry::file_name);

    for entry in entries {
        let path = entry.path();
        if fs::symlink_metadata(&path)
            .map_err(|error| format!("stat {}: {error}", path.display()))?
            .file_type()
            .is_symlink()
        {
            return Err(format!("symlink in package tree: {}", path.display()));
        }
        if path.is_dir() {
            collect_files_inner(&path, result)?;
        } else if path.is_file()
            && path.file_name().and_then(|name| name.to_str()) != Some("MANIFEST.txt")
        {
            result.push(path);
        }
    }
    Ok(())
}

fn sha256(root: &Path, path: &Path) -> Result<String, String> {
    if !command_available("sha256sum", root) {
        return Err("sha256sum is required for package manifests".to_owned());
    }
    let output = run_checked("sha256sum", &[path.to_string_lossy().as_ref()], root)?;
    output
        .stdout
        .split_whitespace()
        .next()
        .map(str::to_owned)
        .ok_or_else(|| format!("sha256sum produced no digest for {}", path.display()))
}

fn create_deterministic_tar(root: &Path, stage: &Path, tar_path: &Path) -> Result<(), String> {
    if !command_available("tar", root) {
        return Err("tar is required to create the deterministic package archive".to_owned());
    }
    let tar_path_text = tar_path.to_string_lossy();
    let stage_text = stage.to_string_lossy();
    // Archive the stage contents from a canonical "." root. The caller's
    // chosen output directory name is intentionally excluded from tar headers
    // so byte-identical package payloads produce byte-identical archives.
    let args = [
        "--sort=name",
        "--mtime=@0",
        "--owner=0",
        "--group=0",
        "--numeric-owner",
        "--format=ustar",
        "-cf",
        tar_path_text.as_ref(),
        "-C",
        stage_text.as_ref(),
        ".",
    ];
    run_checked("tar", &args, root)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        archive_path, collect_files, create_deterministic_tar, runtime_binaries_from_list,
        workspace_version,
    };
    use std::fs;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("nuraloumi-xtask-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    #[test]
    fn workspace_version_reads_workspace_package_only() {
        let root = temp_dir("version");
        fs::write(
            root.join("Cargo.toml"),
            "[package]\nversion=\"9.9.9\"\n[workspace.package]\nversion = \"0.1.0\"\n",
        )
        .expect("write manifest");
        assert_eq!(workspace_version(&root).expect("version"), "0.1.0");
        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn binary_list_includes_new_surfaces() {
        let list = "nuraloumi-menu\nnuraloumi-panel\nnuraloumi-probe\n\
                    nuraloumi-thumbnail-helper\nnuraloumi-feeds\n";
        assert_eq!(
            runtime_binaries_from_list(list).unwrap(),
            vec![
                "nuraloumi-feeds",
                "nuraloumi-menu",
                "nuraloumi-panel",
                "nuraloumi-probe",
                "nuraloumi-thumbnail-helper"
            ]
        );
    }

    #[test]
    fn binary_list_rejects_missing_baseline_and_unsafe_names() {
        assert!(runtime_binaries_from_list("nuraloumi-panel\n").is_err());
        assert!(runtime_binaries_from_list("nuraloumi-panel\n../escape\n").is_err());
    }

    #[test]
    fn deterministic_tar_does_not_depend_on_stage_directory_name() {
        let root = temp_dir("deterministic-tar");
        let stage_a = root.join("stage-a");
        let stage_b = root.join("different-stage-name");
        for stage in [&stage_a, &stage_b] {
            fs::create_dir_all(stage.join("bin")).expect("create stage tree");
            fs::write(stage.join("README.md"), b"same package\n").expect("write readme");
            fs::write(stage.join("bin/nuraloumi-panel"), b"same binary bytes")
                .expect("write binary");
        }

        let tar_a = root.join("a.tar");
        let tar_b = root.join("b.tar");
        create_deterministic_tar(&root, &stage_a, &tar_a).expect("tar stage a");
        create_deterministic_tar(&root, &stage_b, &tar_b).expect("tar stage b");
        assert_eq!(
            fs::read(&tar_a).expect("read tar a"),
            fs::read(&tar_b).expect("read tar b")
        );

        fs::remove_dir_all(root).expect("cleanup");
    }

    #[test]
    fn archive_path_appends_instead_of_replacing_version_suffix() {
        assert_eq!(
            archive_path(PathBuf::from("/tmp/nuraloumi-0.1.0-x86").as_path()),
            PathBuf::from("/tmp/nuraloumi-0.1.0-x86.tar")
        );
    }

    #[test]
    fn collect_files_is_recursive_and_excludes_manifest() {
        let root = temp_dir("collect");
        fs::create_dir_all(root.join("a")).expect("create nested");
        fs::write(root.join("z"), "z").expect("write z");
        fs::write(root.join("a/x"), "x").expect("write x");
        fs::write(root.join("MANIFEST.txt"), "ignored").expect("write manifest");
        let mut relative = collect_files(&root)
            .expect("collect")
            .into_iter()
            .map(|path| path.strip_prefix(&root).expect("relative").to_path_buf())
            .collect::<Vec<_>>();
        relative.sort();
        assert_eq!(relative, vec![PathBuf::from("a/x"), PathBuf::from("z")]);
        fs::remove_dir_all(root).expect("cleanup");
    }
}
