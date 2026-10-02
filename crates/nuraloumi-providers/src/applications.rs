use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(4), 16 * 1024);
const MAX_DESKTOP_FILES: usize = 4096;
const MAX_DESKTOP_FILE_BYTES: u64 = 256 * 1024;
const MAX_RECURSION_DEPTH: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationEntry {
    pub id: String,
    pub name: String,
    pub generic_name: Option<String>,
    pub keywords: Vec<String>,
    pub icon: Option<String>,
    pub desktop_file: PathBuf,
    pub launchable: bool,
}

impl ApplicationEntry {
    pub fn search_text(&self) -> String {
        let mut text = self.name.clone();
        if let Some(generic_name) = &self.generic_name {
            text.push(' ');
            text.push_str(generic_name);
        }
        for keyword in &self.keywords {
            text.push(' ');
            text.push_str(keyword);
        }
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplicationSnapshot {
    pub meta: SnapshotMeta,
    pub applications: Vec<ApplicationEntry>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplicationAction {
    Launch { id: String },
}

pub struct ApplicationProvider<R> {
    runner: R,
    roots: Vec<PathBuf>,
    timestamp_override: Option<u64>,
}

impl ApplicationProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self::new(SystemCommandRunner, system_application_roots())
    }
}

impl<R> ApplicationProvider<R> {
    pub fn new(runner: R, roots: Vec<PathBuf>) -> Self {
        Self {
            runner,
            roots,
            timestamp_override: None,
        }
    }

    pub fn with_timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp_override = timestamp;
        self
    }

    pub fn into_runner(self) -> R {
        self.runner
    }

    fn discover(&self) -> (Vec<ApplicationEntry>, Vec<String>) {
        let mut by_id = BTreeMap::new();
        let mut issues = Vec::new();
        let mut visited = 0usize;

        for root in &self.roots {
            scan_root(root, root, 0, &mut visited, &mut by_id, &mut issues);
            if visited >= MAX_DESKTOP_FILES {
                issues.push(format!(
                    "application scan stopped at bounded limit of {MAX_DESKTOP_FILES} desktop files"
                ));
                break;
            }
        }

        let mut applications = by_id.into_values().collect::<Vec<_>>();
        applications.sort_by_key(|entry| (entry.name.to_ascii_lowercase(), entry.id.clone()));
        (applications, issues)
    }
}

impl<R> Provider for ApplicationProvider<R> {
    type Snapshot = ApplicationSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let (applications, issues) = self.discover();
        let health = if applications.is_empty() {
            Health::Unavailable
        } else if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        Ok(ApplicationSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, "xdg-desktop-entries"),
            applications,
            issues,
        })
    }
}

impl<R: CommandRunner> ActionProvider<ApplicationAction> for ApplicationProvider<R> {
    fn execute(&mut self, action: ApplicationAction) -> Result<ActionResult, ProviderError> {
        let ApplicationAction::Launch { id } = action;
        validate_desktop_id(&id)?;
        let (applications, _) = self.discover();
        let entry = applications
            .into_iter()
            .find(|entry| entry.id == id)
            .ok_or_else(|| {
                ProviderError::unavailable(format!("application {id:?} is not present"))
            })?;
        if !entry.launchable {
            return Err(ProviderError::unavailable(format!(
                "application {:?} has no supported launch command",
                entry.name
            )));
        }

        // Invoke gio with exact argv. The selected desktop file remains the
        // authority for Desktop Entry Exec field-code expansion; no shell
        // interpolation is introduced by NuraLoumi.
        let path = entry
            .desktop_file
            .to_str()
            .ok_or_else(|| ProviderError::parse("desktop file path is not valid UTF-8"))?;
        let spec = CommandSpec::new("gio").args(["launch".to_owned(), path.to_owned()]);
        let output = self.runner.run(&spec, ACTION_LIMITS)?;
        if output.status != 0 {
            return Err(ProviderError::backend(format!(
                "gio launch exited with status {}",
                output.status
            )));
        }
        Ok(ActionResult::executed(format!(
            "launched application {}",
            entry.name
        )))
    }
}

fn system_application_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(data_home) = env::var_os("XDG_DATA_HOME") {
        roots.push(PathBuf::from(data_home).join("applications"));
    } else if let Some(home) = env::var_os("HOME") {
        roots.push(PathBuf::from(home).join(".local/share/applications"));
    }

    let data_dirs = env::var_os("XDG_DATA_DIRS")
        .and_then(|value| value.into_string().ok())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".to_owned());
    for directory in data_dirs.split(':').filter(|value| !value.is_empty()) {
        roots.push(PathBuf::from(directory).join("applications"));
    }
    roots
}

fn scan_root(
    root: &Path,
    directory: &Path,
    depth: usize,
    visited: &mut usize,
    by_id: &mut BTreeMap<String, ApplicationEntry>,
    issues: &mut Vec<String>,
) {
    if depth > MAX_RECURSION_DEPTH || *visited >= MAX_DESKTOP_FILES {
        return;
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return,
        Err(error) => {
            issues.push(format!("cannot read {}: {error}", directory.display()));
            return;
        }
    };

    let mut paths = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect::<Vec<_>>();
    paths.sort();

    for path in paths {
        if *visited >= MAX_DESKTOP_FILES {
            return;
        }
        let file_type = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata.file_type(),
            Err(error) => {
                issues.push(format!("cannot inspect {}: {error}", path.display()));
                continue;
            }
        };
        if file_type.is_symlink() {
            continue;
        }
        if file_type.is_dir() {
            scan_root(root, &path, depth + 1, visited, by_id, issues);
            continue;
        }
        if path.extension().and_then(|value| value.to_str()) != Some("desktop") {
            continue;
        }
        *visited += 1;
        match parse_desktop_file(root, &path) {
            Ok(Some(entry)) => {
                by_id.entry(entry.id.clone()).or_insert(entry);
            }
            Ok(None) => {}
            Err(error) => issues.push(format!("{}: {error}", path.display())),
        }
    }
}

fn parse_desktop_file(root: &Path, path: &Path) -> Result<Option<ApplicationEntry>, ProviderError> {
    let metadata =
        fs::metadata(path).map_err(|error| ProviderError::from_io("stat desktop file", &error))?;
    if metadata.len() > MAX_DESKTOP_FILE_BYTES {
        return Err(ProviderError::parse("desktop file exceeds size limit"));
    }
    let text = fs::read_to_string(path)
        .map_err(|error| ProviderError::from_io("read desktop file", &error))?;
    let fields = parse_desktop_entry_fields(&text)?;

    if fields
        .get("Type")
        .is_some_and(|value| value != "Application")
        || parse_bool(fields.get("Hidden")) == Some(true)
        || parse_bool(fields.get("NoDisplay")) == Some(true)
    {
        return Ok(None);
    }
    let Some(name) = fields.get("Name").filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let id = desktop_id(root, path)?;
    let exec = fields.get("Exec");
    let launchable = exec
        .map(|value| validate_exec(value))
        .transpose()?
        .is_some()
        && parse_bool(fields.get("Terminal")) != Some(true);

    Ok(Some(ApplicationEntry {
        id,
        name: name.clone(),
        generic_name: fields.get("GenericName").cloned(),
        keywords: fields
            .get("Keywords")
            .map(|value| {
                value
                    .split(';')
                    .filter(|value| !value.trim().is_empty())
                    .map(|value| value.trim().to_owned())
                    .take(32)
                    .collect()
            })
            .unwrap_or_default(),
        icon: fields.get("Icon").cloned(),
        desktop_file: path.to_path_buf(),
        launchable,
    }))
}

fn parse_desktop_entry_fields(text: &str) -> Result<BTreeMap<String, String>, ProviderError> {
    let mut fields = BTreeMap::new();
    let mut in_desktop_entry = false;
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            in_desktop_entry = line == "[Desktop Entry]";
            continue;
        }
        if !in_desktop_entry {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(ProviderError::parse("malformed desktop entry field"));
        };
        if key.contains('[') {
            continue;
        }
        fields
            .entry(key.to_owned())
            .or_insert_with(|| value.to_owned());
    }
    Ok(fields)
}

fn validate_exec(exec: &str) -> Result<Option<()>, ProviderError> {
    let tokens = tokenize_exec(exec)?;
    if tokens.is_empty() {
        return Ok(None);
    }
    let program = &tokens[0];
    if program.is_empty() || program.contains(['\0', '\n', '\r']) {
        return Err(ProviderError::parse("invalid desktop Exec program"));
    }
    for token in &tokens {
        validate_field_codes(token)?;
    }
    Ok(Some(()))
}

fn tokenize_exec(input: &str) -> Result<Vec<String>, ProviderError> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => quoted = !quoted,
            ch if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if escaped || quoted {
        return Err(ProviderError::parse("unterminated quoting in desktop Exec"));
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    Ok(tokens)
}

fn validate_field_codes(token: &str) -> Result<(), ProviderError> {
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '%' {
            continue;
        }
        let Some(code) = chars.next() else {
            return Err(ProviderError::parse("dangling % in desktop Exec"));
        };
        if !matches!(code, '%' | 'f' | 'F' | 'u' | 'U' | 'i' | 'c' | 'k') {
            return Err(ProviderError::parse(format!(
                "unsupported desktop Exec field code %{code}"
            )));
        }
    }
    Ok(())
}

fn parse_bool(value: Option<&String>) -> Option<bool> {
    match value.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "true" || value == "1" => Some(true),
        Some(value) if value == "false" || value == "0" => Some(false),
        _ => None,
    }
}

fn desktop_id(root: &Path, path: &Path) -> Result<String, ProviderError> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| ProviderError::parse("desktop file is outside application root"))?;
    let id = relative
        .to_str()
        .ok_or_else(|| ProviderError::parse("desktop file name is not valid UTF-8"))?
        .replace('/', "-");
    validate_desktop_id(&id)?;
    Ok(id)
}

fn validate_desktop_id(id: &str) -> Result<(), ProviderError> {
    if id.is_empty()
        || id.len() > 512
        || !id.ends_with(".desktop")
        || id.contains(['\0', '\n', '\r', '/'])
    {
        return Err(ProviderError::parse("invalid desktop application id"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_dir() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = env::temp_dir().join(format!(
            "nuraloumi-applications-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&path).expect("fixture dir");
        path
    }

    #[test]
    fn desktop_entries_are_filtered_and_sorted() {
        let root = fixture_dir();
        fs::write(
            root.join("zeta.desktop"),
            "[Desktop Entry]\nType=Application\nName=Zeta\nGenericName=Editor\nKeywords=text;code;\nExec=zeta %U\n",
        )
        .unwrap();
        fs::write(
            root.join("alpha.desktop"),
            "[Desktop Entry]\nType=Application\nName=Alpha\nExec=alpha --new-window %u\n",
        )
        .unwrap();
        fs::write(
            root.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nNoDisplay=true\nExec=hidden\n",
        )
        .unwrap();

        let snapshot =
            ApplicationProvider::new(FixtureCommandRunner::default(), vec![root.clone()])
                .with_timestamp(Some(7))
                .snapshot()
                .unwrap();
        assert_eq!(snapshot.meta.health, Health::Healthy);
        assert_eq!(
            snapshot
                .applications
                .iter()
                .map(|entry| entry.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Alpha", "Zeta"]
        );
        assert!(snapshot.applications[1].search_text().contains("code"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn unsupported_exec_field_codes_fail_closed() {
        assert!(validate_exec("demo %Z").is_err());
        assert!(validate_exec("demo \"unterminated").is_err());
        assert!(validate_exec("demo --file=%u").is_ok());
    }

    #[test]
    fn launch_uses_exact_gio_argv_for_discovered_id() {
        let root = fixture_dir();
        let desktop = root.join("safe.desktop");
        fs::write(
            &desktop,
            "[Desktop Entry]\nType=Application\nName=Safe App\nExec=safe-app %U\n",
        )
        .unwrap();
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("gio")
                .args(["launch".to_owned(), desktop.to_string_lossy().into_owned()]),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        let mut provider = ApplicationProvider::new(runner, vec![root.clone()]);
        let result = provider
            .execute(ApplicationAction::Launch {
                id: "safe.desktop".to_owned(),
            })
            .unwrap();
        assert!(result.executed);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn terminal_entries_are_visible_but_not_launchable_in_r1() {
        let root = fixture_dir();
        fs::write(
            root.join("terminal.desktop"),
            "[Desktop Entry]\nType=Application\nName=Terminal App\nTerminal=true\nExec=inside-terminal\n",
        )
        .unwrap();
        let snapshot =
            ApplicationProvider::new(FixtureCommandRunner::default(), vec![root.clone()])
                .snapshot()
                .unwrap();
        assert_eq!(snapshot.applications.len(), 1);
        assert!(!snapshot.applications[0].launchable);
        let _ = fs::remove_dir_all(root);
    }
}
