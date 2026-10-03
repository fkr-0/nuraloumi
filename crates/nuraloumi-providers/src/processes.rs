use crate::common::{Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::collections::BinaryHeap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_PROCESSES: usize = 256;
const MAX_ISSUES: usize = 32;
const MAX_STAT_BYTES: u64 = 16 * 1024;
const MAX_STATUS_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessEntry {
    pub id: String,
    pub pid: u32,
    pub label: String,
    pub state: String,
    pub cpu_percent: Option<u8>,
    pub memory_mib: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSnapshot {
    pub meta: SnapshotMeta,
    pub processes: Vec<ProcessEntry>,
    pub issues: Vec<String>,
}

pub struct ProcessProvider {
    root: PathBuf,
    source: String,
    timestamp_override: Option<u64>,
}

impl ProcessProvider {
    pub fn system() -> Self {
        Self::new("/proc").with_source("procfs:/proc")
    }

    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            source: "procfs".to_owned(),
            timestamp_override: None,
        }
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn with_timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp_override = timestamp;
        self
    }

    fn scan(&self) -> (Vec<ProcessEntry>, Vec<String>) {
        // Keep the lowest MAX_PROCESSES PIDs without retaining an unbounded
        // directory-sized vector. procfs iteration order is not stable, so
        // truncating before ordering would make the visible task set depend on
        // filesystem enumeration order.
        let mut pids = BinaryHeap::with_capacity(MAX_PROCESSES);
        let mut issues = Vec::new();
        let entries = match fs::read_dir(&self.root) {
            Ok(entries) => entries,
            Err(error) => {
                push_issue(
                    &mut issues,
                    format!("cannot read {}: {error}", self.root.display()),
                );
                return (Vec::new(), issues);
            }
        };

        for entry in entries.flatten() {
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(pid) = name.parse::<u32>() else {
                continue;
            };
            if pids.len() < MAX_PROCESSES {
                pids.push(pid);
            } else if pids.peek().is_some_and(|largest| pid < *largest) {
                pids.pop();
                pids.push(pid);
            }
        }
        let mut pids = pids.into_vec();
        pids.sort_unstable();

        let mut processes = Vec::with_capacity(pids.len());
        for pid in pids {
            match read_process(&self.root, pid) {
                Ok(Some(process)) => processes.push(process),
                Ok(None) => {}
                Err(error) => push_issue(&mut issues, format!("pid {pid}: {error}")),
            }
        }

        processes.sort_by(|a, b| {
            b.memory_mib
                .unwrap_or_default()
                .cmp(&a.memory_mib.unwrap_or_default())
                .then_with(|| {
                    a.label
                        .to_ascii_lowercase()
                        .cmp(&b.label.to_ascii_lowercase())
                })
                .then_with(|| a.pid.cmp(&b.pid))
        });
        (processes, issues)
    }
}

impl Provider for ProcessProvider {
    type Snapshot = ProcessSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = self.timestamp_override.unwrap_or_default();
        let (processes, issues) = self.scan();
        let health = if processes.is_empty() {
            Health::Unavailable
        } else if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        Ok(ProcessSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, self.source.clone()),
            processes,
            issues,
        })
    }
}

fn read_process(root: &Path, pid: u32) -> Result<Option<ProcessEntry>, ProviderError> {
    let directory = root.join(pid.to_string());
    let stat = match read_limited(&directory.join("stat"), MAX_STAT_BYTES) {
        Ok(value) => value,
        Err(error) if is_transient(&error) => return Ok(None),
        Err(error) => return Err(error),
    };
    let (label, state) = parse_stat(&stat)?;
    let memory_mib = match read_limited(&directory.join("status"), MAX_STATUS_BYTES) {
        Ok(status) => parse_rss_mib(&status),
        Err(error) if is_transient(&error) => None,
        Err(error) => return Err(error),
    };

    Ok(Some(ProcessEntry {
        id: format!("pid:{pid}"),
        pid,
        label,
        state,
        cpu_percent: None,
        memory_mib,
    }))
}

fn read_limited(path: &Path, max_bytes: u64) -> Result<String, ProviderError> {
    let file = fs::File::open(path)
        .map_err(|error| ProviderError::from_io(&format!("open {}", path.display()), &error))?;
    let mut bytes = Vec::new();
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ProviderError::from_io(&format!("read {}", path.display()), &error))?;
    if bytes.len() as u64 > max_bytes {
        return Err(ProviderError::parse(format!(
            "{} exceeds bounded read limit",
            path.display()
        )));
    }
    String::from_utf8(bytes)
        .map_err(|_| ProviderError::parse(format!("{} is not UTF-8", path.display())))
}

fn parse_stat(stat: &str) -> Result<(String, String), ProviderError> {
    let open = stat
        .find('(')
        .ok_or_else(|| ProviderError::parse("missing process stat command"))?;
    let close = stat
        .rfind(") ")
        .ok_or_else(|| ProviderError::parse("missing process stat state"))?;
    if close <= open + 1 {
        return Err(ProviderError::parse("empty process stat command"));
    }
    let label = stat[open + 1..close].to_owned();
    let state_code = stat[close + 2..]
        .chars()
        .next()
        .ok_or_else(|| ProviderError::parse("missing process state code"))?;
    Ok((label, process_state_label(state_code).to_owned()))
}

fn process_state_label(code: char) -> &'static str {
    match code {
        'R' => "Running",
        'S' => "Sleeping",
        'D' => "Disk sleep",
        'T' | 't' => "Stopped",
        'Z' => "Zombie",
        'I' => "Idle",
        'X' | 'x' => "Dead",
        'P' => "Parked",
        _ => "Unknown",
    }
}

fn parse_rss_mib(status: &str) -> Option<u32> {
    let kb = status.lines().find_map(|line| {
        let value = line.strip_prefix("VmRSS:")?.trim();
        value.split_whitespace().next()?.parse::<u64>().ok()
    })?;
    Some(((kb.saturating_add(1023)) / 1024).min(u64::from(u32::MAX)) as u32)
}

fn is_transient(error: &ProviderError) -> bool {
    let text = error.to_string();
    text.contains("No such file or directory") || text.contains("Permission denied")
}

fn push_issue(issues: &mut Vec<String>, issue: String) {
    if issues.len() < MAX_ISSUES {
        issues.push(issue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::Provider;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn fixture_root() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "nuraloumi-processes-{}-{nonce}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("fixture root");
        root
    }

    fn write_process(root: &Path, pid: u32, name: &str, state: char, rss_kib: u32) {
        let dir = root.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("stat"),
            format!("{pid} ({name}) {state} 1 2 3 4 5 6 7 8 9 10\n"),
        )
        .unwrap();
        fs::write(
            dir.join("status"),
            format!("Name:\t{name}\nVmRSS:\t{rss_kib} kB\n"),
        )
        .unwrap();
    }

    #[test]
    fn process_snapshot_is_bounded_sorted_and_read_only() {
        let root = fixture_root();
        write_process(&root, 20, "small task", 'S', 1024);
        write_process(&root, 10, "large task", 'R', 4097);
        let mut provider = ProcessProvider::new(&root)
            .with_source("fixture:proc")
            .with_timestamp(Some(7));
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.meta.timestamp_ms, 7);
        assert_eq!(snapshot.processes.len(), 2);
        assert_eq!(snapshot.processes[0].id, "pid:10");
        assert_eq!(snapshot.processes[0].state, "Running");
        assert_eq!(snapshot.processes[0].memory_mib, Some(5));
        assert_eq!(snapshot.processes[1].label, "small task");
        assert!(snapshot
            .processes
            .iter()
            .all(|entry| entry.cpu_percent.is_none()));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn stat_parser_handles_spaces_and_parentheses_in_names() {
        let (name, state) = parse_stat("42 (hello (world)) S 1 2 3").unwrap();
        assert_eq!(name, "hello (world)");
        assert_eq!(state, "Sleeping");
    }

    #[test]
    fn disappearing_or_incomplete_entries_do_not_panic() {
        let root = fixture_root();
        fs::create_dir_all(root.join("12")).unwrap();
        write_process(&root, 11, "kept", 'R', 2048);
        let mut provider = ProcessProvider::new(&root);
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.processes.len(), 1);
        assert_eq!(snapshot.processes[0].pid, 11);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn process_cap_selects_lowest_pids_independent_of_directory_order() {
        let root = fixture_root();
        for pid in (1..=(MAX_PROCESSES as u32 + 8)).rev() {
            write_process(&root, pid, &format!("task-{pid}"), 'S', pid);
        }

        let mut provider = ProcessProvider::new(&root);
        let snapshot = provider.snapshot().unwrap();
        let mut selected: Vec<u32> = snapshot.processes.iter().map(|entry| entry.pid).collect();
        selected.sort_unstable();

        assert_eq!(selected.len(), MAX_PROCESSES);
        assert_eq!(selected.first(), Some(&1));
        assert_eq!(selected.last(), Some(&(MAX_PROCESSES as u32)));
        assert!(!selected.contains(&(MAX_PROCESSES as u32 + 1)));

        let _ = fs::remove_dir_all(root);
    }
}
