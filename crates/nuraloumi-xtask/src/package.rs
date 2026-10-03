use crate::process::{command_available, run_checked};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

const RUNTIME_BINARIES: &[&str] = &[
    "nuraloumi-panel",
    "nuraloumi-menu",
    "nuraloumi-thumbnail-helper",
    "nuraloumi-probe",
];

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
    for name in RUNTIME_BINARIES {
        let source = binaries_dir.join(name);
        if !source.is_file() {
            return Err(format!(
                "required runtime release binary missing: {}; run the complete target release build first",
                source.display()
            ));
        }
        let destination = bin_dir.join(name);
        fs::copy(&source, &destination).map_err(|error| {
            format!(
                "copy runtime binary {} -> {}: {error}",
                source.display(),
                destination.display()
            )
        })?;
        staged_bins.push((*name).to_owned());
    }

    copy_required(&root.join("packaging/README.md"), &stage.join("README.md"))?;
    copy_required(&root.join("packaging/LICENSE"), &stage.join("LICENSE"))?;
    copy_required(
        &root.join("packaging/nuraloumi.toml.example"),
        &config_dir.join("nuraloumi.toml.example"),
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
        archive_path, collect_files, create_deterministic_tar, workspace_version, RUNTIME_BINARIES,
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
    fn runtime_package_requires_thumbnail_helper() {
        assert_eq!(
            RUNTIME_BINARIES,
            [
                "nuraloumi-panel",
                "nuraloumi-menu",
                "nuraloumi-thumbnail-helper",
                "nuraloumi-probe",
            ]
        );
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
