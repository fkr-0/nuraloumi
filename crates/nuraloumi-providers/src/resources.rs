use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::collections::{BTreeMap, BinaryHeap};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const MAX_PROC_TEXT_BYTES: u64 = 256 * 1024;
const MAX_PROCESS_STAT_BYTES: u64 = 16 * 1024;
const MAX_PROCESS_STATUS_BYTES: u64 = 64 * 1024;
const MAX_PROCESS_SCAN: usize = 512;
const MAX_TOP_PROCESSES: usize = 10;
const MAX_THERMAL_ZONES: usize = 16;
const MAX_ISSUES: usize = 32;
const DF_TIMEOUT: Duration = Duration::from_secs(2);
const DF_OUTPUT_LIMIT: usize = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuUsage {
    pub id: String,
    pub percent: Option<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStats {
    pub total_kib: u64,
    pub available_kib: u64,
    pub used_kib: u64,
    pub used_percent: u8,
    pub cached_kib: u64,
    pub buffers_kib: u64,
    pub swap_total_kib: u64,
    pub swap_free_kib: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilesystemStats {
    pub mount: String,
    pub total_kib: u64,
    pub used_kib: u64,
    pub available_kib: u64,
    pub used_percent: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceProcess {
    pub pid: u32,
    pub label: String,
    pub cpu_percent: Option<u16>,
    pub memory_mib: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkIoStats {
    pub received_bytes_total: u64,
    pub transmitted_bytes_total: u64,
    pub received_bytes_per_sec: Option<u64>,
    pub transmitted_bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiskIoStats {
    pub read_bytes_total: u64,
    pub written_bytes_total: u64,
    pub read_bytes_per_sec: Option<u64>,
    pub written_bytes_per_sec: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TemperatureReading {
    pub label: String,
    pub milli_celsius: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceSnapshot {
    pub meta: SnapshotMeta,
    pub cpu_percent: Option<u8>,
    pub cpu_cores: Vec<CpuUsage>,
    pub memory: Option<MemoryStats>,
    pub filesystem: Option<FilesystemStats>,
    pub top_processes: Vec<ResourceProcess>,
    pub network: Option<NetworkIoStats>,
    pub disk_io: Option<DiskIoStats>,
    pub temperatures: Vec<TemperatureReading>,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone)]
struct CpuCounter {
    id: String,
    total: u64,
    idle: u64,
}

#[derive(Debug, Clone, Copy, Default)]
struct ByteCounters {
    first: u64,
    second: u64,
}

#[derive(Debug, Clone)]
struct ProcessObservation {
    pid: u32,
    label: String,
    ticks: u64,
    memory_mib: Option<u32>,
}

#[derive(Debug)]
struct CounterState {
    sampled_at: Instant,
    cpu: Vec<CpuCounter>,
    network: Option<ByteCounters>,
    disk: Option<ByteCounters>,
    process_ticks: BTreeMap<u32, u64>,
}

pub struct ResourceProvider<R = SystemCommandRunner> {
    runner: R,
    proc_root: PathBuf,
    sys_root: PathBuf,
    mount: String,
    source: String,
    timestamp_override: Option<u64>,
    interval_override: Option<Duration>,
    previous: Option<CounterState>,
}

impl ResourceProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self::new(SystemCommandRunner)
    }
}

impl<R> ResourceProvider<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            proc_root: PathBuf::from("/proc"),
            sys_root: PathBuf::from("/sys"),
            mount: "/".to_owned(),
            source: "procfs+sysfs+df".to_owned(),
            timestamp_override: None,
            interval_override: None,
            previous: None,
        }
    }

    pub fn with_roots(
        mut self,
        proc_root: impl Into<PathBuf>,
        sys_root: impl Into<PathBuf>,
    ) -> Self {
        self.proc_root = proc_root.into();
        self.sys_root = sys_root.into();
        self
    }

    pub fn with_mount(mut self, mount: impl Into<String>) -> Self {
        self.mount = mount.into();
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }

    pub fn with_timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp_override = timestamp;
        self
    }

    pub fn with_sample_interval(mut self, interval: Option<Duration>) -> Self {
        self.interval_override = interval;
        self
    }
}

impl<R> Provider for ResourceProvider<R>
where
    R: CommandRunner,
{
    type Snapshot = ResourceSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let sampled_at = Instant::now();
        let mut issues = Vec::new();

        let cpu = match read_cpu_counters(&self.proc_root.join("stat")) {
            Ok(value) => value,
            Err(error) => {
                push_issue(&mut issues, error.diagnostic());
                Vec::new()
            }
        };
        let memory = match read_limited(&self.proc_root.join("meminfo"), MAX_PROC_TEXT_BYTES) {
            Ok(text) => match parse_meminfo(&text) {
                Some(value) => Some(value),
                None => {
                    push_issue(&mut issues, "meminfo lacks required fields".to_owned());
                    None
                }
            },
            Err(error) => {
                push_issue(&mut issues, error.diagnostic());
                None
            }
        };
        let network_counters =
            match read_limited(&self.proc_root.join("net/dev"), MAX_PROC_TEXT_BYTES) {
                Ok(text) => match parse_net_dev(&text) {
                    Some(value) => Some(value),
                    None => {
                        push_issue(&mut issues, "no non-loopback network counters".to_owned());
                        None
                    }
                },
                Err(error) => {
                    push_issue(&mut issues, error.diagnostic());
                    None
                }
            };
        let disk_counters =
            match read_limited(&self.proc_root.join("diskstats"), MAX_PROC_TEXT_BYTES) {
                Ok(text) => match parse_diskstats(&text) {
                    Some(value) => Some(value),
                    None => {
                        push_issue(&mut issues, "no physical disk counters".to_owned());
                        None
                    }
                },
                Err(error) => {
                    push_issue(&mut issues, error.diagnostic());
                    None
                }
            };
        let filesystem = match filesystem_stats(&mut self.runner, &self.mount) {
            Ok(value) => Some(value),
            Err(error) => {
                push_issue(&mut issues, error.diagnostic());
                None
            }
        };
        let temperatures = read_temperatures(&self.sys_root, &mut issues);
        let process_observations = scan_processes(&self.proc_root, &mut issues);

        let interval = self.interval_override.or_else(|| {
            self.previous
                .as_ref()
                .map(|previous| sampled_at.saturating_duration_since(previous.sampled_at))
        });
        let previous_cpu = self.previous.as_ref().map(|state| &state.cpu);
        let cpu_percent = cpu
            .iter()
            .find(|counter| counter.id == "cpu")
            .and_then(|current| {
                previous_cpu
                    .and_then(|previous| previous.iter().find(|counter| counter.id == "cpu"))
                    .and_then(|previous| cpu_delta_percent(previous, current))
            });
        let cpu_cores = cpu
            .iter()
            .filter(|counter| counter.id != "cpu")
            .map(|current| CpuUsage {
                id: current.id.clone(),
                percent: previous_cpu
                    .and_then(|previous| {
                        previous.iter().find(|candidate| candidate.id == current.id)
                    })
                    .and_then(|previous| cpu_delta_percent(previous, current)),
            })
            .collect::<Vec<_>>();

        let network = network_counters.map(|current| {
            let previous = self.previous.as_ref().and_then(|state| state.network);
            NetworkIoStats {
                received_bytes_total: current.first,
                transmitted_bytes_total: current.second,
                received_bytes_per_sec: previous.zip(interval).and_then(|(previous, interval)| {
                    rate_per_second(previous.first, current.first, interval)
                }),
                transmitted_bytes_per_sec: previous.zip(interval).and_then(
                    |(previous, interval)| {
                        rate_per_second(previous.second, current.second, interval)
                    },
                ),
            }
        });
        let disk_io = disk_counters.map(|current| {
            let previous = self.previous.as_ref().and_then(|state| state.disk);
            DiskIoStats {
                read_bytes_total: current.first,
                written_bytes_total: current.second,
                read_bytes_per_sec: previous.zip(interval).and_then(|(previous, interval)| {
                    rate_per_second(previous.first, current.first, interval)
                }),
                written_bytes_per_sec: previous.zip(interval).and_then(|(previous, interval)| {
                    rate_per_second(previous.second, current.second, interval)
                }),
            }
        });

        let aggregate_total_delta = current_total_delta(
            self.previous.as_ref().map(|state| state.cpu.as_slice()),
            &cpu,
        );
        let core_count = cpu
            .iter()
            .filter(|counter| counter.id != "cpu")
            .count()
            .max(1);
        let top_processes = rank_processes(
            &process_observations,
            self.previous.as_ref().map(|state| &state.process_ticks),
            aggregate_total_delta,
            core_count,
        );

        self.previous = Some(CounterState {
            sampled_at,
            cpu,
            network: network_counters,
            disk: disk_counters,
            process_ticks: process_observations
                .iter()
                .map(|process| (process.pid, process.ticks))
                .collect(),
        });

        let any_available = cpu_percent.is_some()
            || !cpu_cores.is_empty()
            || memory.is_some()
            || filesystem.is_some()
            || network.is_some()
            || disk_io.is_some()
            || !temperatures.is_empty()
            || !top_processes.is_empty();
        let health = if !any_available {
            Health::Unavailable
        } else if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };

        Ok(ResourceSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, self.source.clone()),
            cpu_percent,
            cpu_cores,
            memory,
            filesystem,
            top_processes,
            network,
            disk_io,
            temperatures,
            issues,
        })
    }
}

fn read_cpu_counters(path: &Path) -> Result<Vec<CpuCounter>, ProviderError> {
    let text = read_limited(path, MAX_PROC_TEXT_BYTES)?;
    let mut counters = Vec::new();
    for line in text.lines() {
        let mut fields = line.split_whitespace();
        let Some(id) = fields.next() else {
            continue;
        };
        if id != "cpu"
            && !id.strip_prefix("cpu").is_some_and(|suffix| {
                !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
            })
        {
            continue;
        }
        let values = fields
            .take(8)
            .map(|field| field.parse::<u64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| ProviderError::parse("invalid /proc/stat CPU counter"))?;
        if values.len() < 4 {
            return Err(ProviderError::parse("short /proc/stat CPU counter"));
        }
        let idle =
            values.get(3).copied().unwrap_or_default() + values.get(4).copied().unwrap_or_default();
        let total = values.into_iter().fold(0_u64, u64::saturating_add);
        counters.push(CpuCounter {
            id: id.to_owned(),
            total,
            idle,
        });
    }
    if counters.is_empty() {
        return Err(ProviderError::unavailable("no CPU counters in /proc/stat"));
    }
    Ok(counters)
}

fn cpu_delta_percent(previous: &CpuCounter, current: &CpuCounter) -> Option<u8> {
    let total = current.total.checked_sub(previous.total)?;
    if total == 0 {
        return None;
    }
    let idle = current.idle.checked_sub(previous.idle)?;
    let busy = total.saturating_sub(idle);
    Some(((u128::from(busy) * 100 / u128::from(total)).min(100)) as u8)
}

fn current_total_delta(previous: Option<&[CpuCounter]>, current: &[CpuCounter]) -> Option<u64> {
    let previous = previous?.iter().find(|counter| counter.id == "cpu")?;
    let current = current.iter().find(|counter| counter.id == "cpu")?;
    current.total.checked_sub(previous.total)
}

fn parse_meminfo(text: &str) -> Option<MemoryStats> {
    let mut values = BTreeMap::new();
    for line in text.lines() {
        let (key, rest) = line.split_once(':')?;
        let value = rest.split_whitespace().next()?.parse::<u64>().ok()?;
        values.insert(key, value);
    }
    let total = *values.get("MemTotal")?;
    let available = values
        .get("MemAvailable")
        .copied()
        .or_else(|| {
            Some(
                values.get("MemFree").copied().unwrap_or_default()
                    + values.get("Buffers").copied().unwrap_or_default()
                    + values.get("Cached").copied().unwrap_or_default(),
            )
        })?
        .min(total);
    let used = total.saturating_sub(available);
    Some(MemoryStats {
        total_kib: total,
        available_kib: available,
        used_kib: used,
        used_percent: percent(used, total),
        cached_kib: values.get("Cached").copied().unwrap_or_default()
            + values.get("SReclaimable").copied().unwrap_or_default(),
        buffers_kib: values.get("Buffers").copied().unwrap_or_default(),
        swap_total_kib: values.get("SwapTotal").copied().unwrap_or_default(),
        swap_free_kib: values.get("SwapFree").copied().unwrap_or_default(),
    })
}

fn parse_net_dev(text: &str) -> Option<ByteCounters> {
    let mut counters = ByteCounters::default();
    let mut found = false;
    for line in text.lines() {
        let Some((name, values)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let fields = values.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 16 {
            continue;
        }
        let Ok(rx) = fields[0].parse::<u64>() else {
            continue;
        };
        let Ok(tx) = fields[8].parse::<u64>() else {
            continue;
        };
        counters.first = counters.first.saturating_add(rx);
        counters.second = counters.second.saturating_add(tx);
        found = true;
    }
    found.then_some(counters)
}

fn parse_diskstats(text: &str) -> Option<ByteCounters> {
    let mut sectors = ByteCounters::default();
    let mut found = false;
    for line in text.lines() {
        let fields = line.split_whitespace().collect::<Vec<_>>();
        if fields.len() < 10 || !is_whole_block_device(fields[2]) {
            continue;
        }
        let Ok(read_sectors) = fields[5].parse::<u64>() else {
            continue;
        };
        let Ok(write_sectors) = fields[9].parse::<u64>() else {
            continue;
        };
        sectors.first = sectors
            .first
            .saturating_add(read_sectors.saturating_mul(512));
        sectors.second = sectors
            .second
            .saturating_add(write_sectors.saturating_mul(512));
        found = true;
    }
    found.then_some(sectors)
}

fn is_whole_block_device(name: &str) -> bool {
    if let Some(rest) = name.strip_prefix("mmcblk") {
        return !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_digit());
    }
    if let Some(rest) = name.strip_prefix("nvme") {
        let Some((controller, namespace)) = rest.split_once('n') else {
            return false;
        };
        return !controller.is_empty()
            && controller.bytes().all(|byte| byte.is_ascii_digit())
            && !namespace.is_empty()
            && namespace.bytes().all(|byte| byte.is_ascii_digit());
    }
    for prefix in ["sd", "vd", "xvd"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            return !rest.is_empty() && rest.bytes().all(|byte| byte.is_ascii_lowercase());
        }
    }
    false
}

fn filesystem_stats<R: CommandRunner>(
    runner: &mut R,
    mount: &str,
) -> Result<FilesystemStats, ProviderError> {
    let output = runner.run(
        &CommandSpec::new("df").args(["-Pk", mount]),
        CommandLimits::new(DF_TIMEOUT, DF_OUTPUT_LIMIT),
    )?;
    if output.status != 0 {
        return Err(ProviderError::backend(format!(
            "df exited with status {}",
            output.status
        )));
    }
    parse_df(&output.stdout).ok_or_else(|| ProviderError::parse("unexpected df -Pk output"))
}

fn parse_df(text: &str) -> Option<FilesystemStats> {
    let line = text.lines().rfind(|line| !line.trim().is_empty())?;
    let fields = line.split_whitespace().collect::<Vec<_>>();
    if fields.len() < 6 {
        return None;
    }
    let base = fields.len() - 5;
    let total = fields[base].parse::<u64>().ok()?;
    let used = fields[base + 1].parse::<u64>().ok()?;
    let available = fields[base + 2].parse::<u64>().ok()?;
    let used_percent = fields[base + 3]
        .trim_end_matches('%')
        .parse::<u8>()
        .ok()?
        .min(100);
    Some(FilesystemStats {
        mount: fields[base + 4].to_owned(),
        total_kib: total,
        used_kib: used,
        available_kib: available,
        used_percent,
    })
}

fn read_temperatures(sys_root: &Path, issues: &mut Vec<String>) -> Vec<TemperatureReading> {
    let thermal_root = sys_root.join("class/thermal");
    let mut zones = match fs::read_dir(&thermal_root) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with("thermal_zone"))
            })
            .collect::<Vec<_>>(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            push_issue(
                issues,
                format!("cannot read {}: {error}", thermal_root.display()),
            );
            return Vec::new();
        }
    };
    zones.sort();
    zones.truncate(MAX_THERMAL_ZONES);
    let mut readings = Vec::new();
    for zone in zones {
        let temp_text = match read_limited(&zone.join("temp"), 64) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Ok(raw) = temp_text.trim().parse::<i64>() else {
            continue;
        };
        let milli = if raw.unsigned_abs() < 1000 {
            raw.saturating_mul(1000)
        } else {
            raw
        };
        let label = read_limited(&zone.join("type"), 256)
            .ok()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
            .or_else(|| {
                zone.file_name()
                    .and_then(|name| name.to_str())
                    .map(ToOwned::to_owned)
            })
            .unwrap_or_else(|| "thermal".to_owned());
        readings.push(TemperatureReading {
            label,
            milli_celsius: milli.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32,
        });
    }
    readings
}

fn scan_processes(proc_root: &Path, issues: &mut Vec<String>) -> Vec<ProcessObservation> {
    let entries = match fs::read_dir(proc_root) {
        Ok(entries) => entries,
        Err(error) => {
            push_issue(
                issues,
                format!("cannot read {}: {error}", proc_root.display()),
            );
            return Vec::new();
        }
    };
    let mut heap = BinaryHeap::with_capacity(MAX_PROCESS_SCAN);
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(pid) = name.parse::<u32>() else {
            continue;
        };
        if heap.len() < MAX_PROCESS_SCAN {
            heap.push(pid);
        } else if heap.peek().is_some_and(|largest| pid < *largest) {
            heap.pop();
            heap.push(pid);
        }
    }
    let mut pids = heap.into_vec();
    pids.sort_unstable();
    let mut processes = Vec::with_capacity(pids.len());
    for pid in pids {
        let directory = proc_root.join(pid.to_string());
        let stat = match read_limited(&directory.join("stat"), MAX_PROCESS_STAT_BYTES) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some((label, ticks)) = parse_process_stat(&stat) else {
            continue;
        };
        let memory_mib = read_limited(&directory.join("status"), MAX_PROCESS_STATUS_BYTES)
            .ok()
            .and_then(|status| parse_rss_mib(&status));
        processes.push(ProcessObservation {
            pid,
            label,
            ticks,
            memory_mib,
        });
    }
    processes
}

fn parse_process_stat(text: &str) -> Option<(String, u64)> {
    let open = text.find('(')?;
    let close = text.rfind(") ")?;
    if close <= open + 1 {
        return None;
    }
    let label = text[open + 1..close].to_owned();
    let fields = text[close + 2..].split_whitespace().collect::<Vec<_>>();
    let utime = fields.get(11)?.parse::<u64>().ok()?;
    let stime = fields.get(12)?.parse::<u64>().ok()?;
    Some((label, utime.saturating_add(stime)))
}

fn parse_rss_mib(status: &str) -> Option<u32> {
    let kib = status.lines().find_map(|line| {
        let value = line.strip_prefix("VmRSS:")?.trim();
        value.split_whitespace().next()?.parse::<u64>().ok()
    })?;
    Some(((kib.saturating_add(1023)) / 1024).min(u64::from(u32::MAX)) as u32)
}

fn rank_processes(
    observations: &[ProcessObservation],
    previous: Option<&BTreeMap<u32, u64>>,
    total_cpu_delta: Option<u64>,
    core_count: usize,
) -> Vec<ResourceProcess> {
    let mut rows = observations
        .iter()
        .map(|process| {
            let cpu_percent = previous
                .and_then(|previous| previous.get(&process.pid))
                .zip(total_cpu_delta)
                .and_then(|(previous_ticks, total)| {
                    if total == 0 {
                        return None;
                    }
                    let delta = process.ticks.checked_sub(*previous_ticks)?;
                    let value = u128::from(delta)
                        .saturating_mul(core_count as u128)
                        .saturating_mul(100)
                        / u128::from(total);
                    Some(value.min(u128::from(u16::MAX)) as u16)
                });
            ResourceProcess {
                pid: process.pid,
                label: process.label.clone(),
                cpu_percent,
                memory_mib: process.memory_mib,
            }
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        b.cpu_percent
            .unwrap_or_default()
            .cmp(&a.cpu_percent.unwrap_or_default())
            .then_with(|| {
                b.memory_mib
                    .unwrap_or_default()
                    .cmp(&a.memory_mib.unwrap_or_default())
            })
            .then_with(|| {
                a.label
                    .to_ascii_lowercase()
                    .cmp(&b.label.to_ascii_lowercase())
            })
            .then_with(|| a.pid.cmp(&b.pid))
    });
    rows.truncate(MAX_TOP_PROCESSES);
    rows
}

fn rate_per_second(previous: u64, current: u64, interval: Duration) -> Option<u64> {
    let delta = current.checked_sub(previous)?;
    let nanos = interval.as_nanos();
    if nanos == 0 {
        return None;
    }
    let rate = u128::from(delta)
        .saturating_mul(1_000_000_000)
        .checked_div(nanos)?;
    Some(rate.min(u128::from(u64::MAX)) as u64)
}

fn percent(value: u64, total: u64) -> u8 {
    if total == 0 {
        return 0;
    }
    (u128::from(value) * 100 / u128::from(total)).min(100) as u8
}

fn read_limited(path: &Path, max_bytes: u64) -> Result<String, ProviderError> {
    let file = fs::File::open(path)
        .map_err(|error| ProviderError::from_io(&format!("open {}", path.display()), &error))?;
    let mut bytes = Vec::new();
    file.take(max_bytes.saturating_add(1))
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

fn push_issue(issues: &mut Vec<String>, issue: String) {
    if issues.len() < MAX_ISSUES {
        issues.push(issue);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture_roots() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "nuraloumi-resources-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let proc_root = root.join("proc");
        let sys_root = root.join("sys");
        fs::create_dir_all(proc_root.join("net")).unwrap();
        fs::create_dir_all(sys_root.join("class/thermal/thermal_zone0")).unwrap();
        (proc_root, sys_root)
    }

    fn write_process(proc_root: &Path, pid: u32, name: &str, ticks: u64, rss_kib: u64) {
        let dir = proc_root.join(pid.to_string());
        fs::create_dir_all(&dir).unwrap();
        let utime = ticks / 2;
        let stime = ticks.saturating_sub(utime);
        fs::write(
            dir.join("stat"),
            format!("{pid} ({name}) R 1 2 3 4 5 6 7 8 9 10 {utime} {stime} 0 0 0 0\n"),
        )
        .unwrap();
        fs::write(
            dir.join("status"),
            format!("Name:\t{name}\nVmRSS:\t{rss_kib} kB\n"),
        )
        .unwrap();
    }

    fn write_sample(proc_root: &Path, second: bool) {
        let (total_user, total_system, total_idle) = if second {
            (180, 70, 950)
        } else {
            (100, 50, 850)
        };
        fs::write(
            proc_root.join("stat"),
            format!(
                "cpu  {total_user} 0 {total_system} {total_idle} 0 0 0 0\n\
                 cpu0 {} 0 {} {} 0 0 0 0\n\
                 cpu1 {} 0 {} {} 0 0 0 0\n",
                if second { 90 } else { 50 },
                if second { 35 } else { 25 },
                if second { 475 } else { 425 },
                if second { 90 } else { 50 },
                if second { 35 } else { 25 },
                if second { 475 } else { 425 },
            ),
        )
        .unwrap();
        fs::write(
            proc_root.join("meminfo"),
            "MemTotal: 1000000 kB\nMemAvailable: 600000 kB\nMemFree: 100000 kB\nBuffers: 20000 kB\nCached: 200000 kB\nSReclaimable: 10000 kB\nSwapTotal: 500000 kB\nSwapFree: 400000 kB\n",
        )
        .unwrap();
        let (rx, tx) = if second {
            (11_024_u64, 22_048_u64)
        } else {
            (10_000_u64, 20_000_u64)
        };
        fs::write(
            proc_root.join("net/dev"),
            format!(
                "Inter-| Receive | Transmit\n face |bytes packets errs drop fifo frame compressed multicast|bytes packets errs drop fifo colls carrier compressed\n wlan0: {rx} 0 0 0 0 0 0 0 {tx} 0 0 0 0 0 0 0\n lo: 999 0 0 0 0 0 0 0 999 0 0 0 0 0 0 0\n"
            ),
        )
        .unwrap();
        let (read_sectors, write_sectors) = if second {
            (102_u64, 204_u64)
        } else {
            (100_u64, 200_u64)
        };
        fs::write(
            proc_root.join("diskstats"),
            format!(
                "179 0 mmcblk0 1 0 {read_sectors} 0 1 0 {write_sectors} 0 0 0 0\n179 1 mmcblk0p1 1 0 9999 0 1 0 9999 0 0 0 0\n"
            ),
        )
        .unwrap();
        write_process(
            proc_root,
            10,
            "browser",
            if second { 120 } else { 100 },
            50 * 1024,
        );
        write_process(
            proc_root,
            20,
            "shell",
            if second { 105 } else { 100 },
            10 * 1024,
        );
    }

    fn fixture_runner() -> FixtureCommandRunner {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("df").args(["-Pk", "/"]),
            CommandOutput {
                status: 0,
                stdout: "Filesystem 1024-blocks Used Available Capacity Mounted on\n/dev/root 1000000 250000 750000 25% /\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner
    }

    #[test]
    fn second_sample_produces_cpu_network_disk_and_process_rates() {
        let (proc_root, sys_root) = fixture_roots();
        write_sample(&proc_root, false);
        fs::write(
            sys_root.join("class/thermal/thermal_zone0/type"),
            "cpu-thermal\n",
        )
        .unwrap();
        fs::write(sys_root.join("class/thermal/thermal_zone0/temp"), "42000\n").unwrap();

        let mut provider = ResourceProvider::new(fixture_runner())
            .with_roots(&proc_root, &sys_root)
            .with_timestamp(Some(7))
            .with_sample_interval(Some(Duration::from_secs(1)));
        let first = provider.snapshot().unwrap();
        assert_eq!(first.cpu_percent, None);
        assert_eq!(first.cpu_cores.len(), 2);
        assert_eq!(first.memory.as_ref().unwrap().used_percent, 40);
        assert_eq!(first.filesystem.as_ref().unwrap().used_percent, 25);
        assert_eq!(first.network.as_ref().unwrap().received_bytes_per_sec, None);
        assert_eq!(first.disk_io.as_ref().unwrap().read_bytes_per_sec, None);
        assert_eq!(first.temperatures[0].milli_celsius, 42_000);

        write_sample(&proc_root, true);
        let second = provider.snapshot().unwrap();
        assert_eq!(second.meta.timestamp_ms, 7);
        assert_eq!(second.cpu_percent, Some(50));
        assert_eq!(
            second
                .cpu_cores
                .iter()
                .map(|cpu| cpu.percent)
                .collect::<Vec<_>>(),
            vec![Some(50), Some(50)]
        );
        assert_eq!(
            second.network.as_ref().unwrap().received_bytes_per_sec,
            Some(1024)
        );
        assert_eq!(
            second.network.as_ref().unwrap().transmitted_bytes_per_sec,
            Some(2048)
        );
        assert_eq!(
            second.disk_io.as_ref().unwrap().read_bytes_per_sec,
            Some(1024)
        );
        assert_eq!(
            second.disk_io.as_ref().unwrap().written_bytes_per_sec,
            Some(2048)
        );
        assert_eq!(second.top_processes[0].label, "browser");
        assert_eq!(second.top_processes[0].cpu_percent, Some(20));
        assert_eq!(second.top_processes[0].memory_mib, Some(50));

        let root = proc_root.parent().unwrap().to_path_buf();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn parsers_reject_partitions_and_handle_missing_optional_sources() {
        assert!(is_whole_block_device("mmcblk0"));
        assert!(!is_whole_block_device("mmcblk0p1"));
        assert!(is_whole_block_device("sda"));
        assert!(!is_whole_block_device("sda1"));
        assert!(is_whole_block_device("nvme0n1"));
        assert!(!is_whole_block_device("nvme0n1p1"));

        let (proc_root, sys_root) = fixture_roots();
        fs::write(proc_root.join("stat"), "cpu 1 0 0 9 0 0 0 0\n").unwrap();
        fs::write(
            proc_root.join("meminfo"),
            "MemTotal: 1000 kB\nMemAvailable: 500 kB\n",
        )
        .unwrap();
        fs::write(proc_root.join("net/dev"), "Inter-| Receive | Transmit\n").unwrap();
        fs::write(proc_root.join("diskstats"), "").unwrap();
        let mut provider = ResourceProvider::new(fixture_runner())
            .with_roots(&proc_root, &sys_root)
            .with_sample_interval(Some(Duration::from_secs(1)));
        let snapshot = provider.snapshot().unwrap();
        assert!(snapshot.memory.is_some());
        assert!(snapshot.network.is_none());
        assert!(snapshot.disk_io.is_none());
        assert!(snapshot.temperatures.is_empty());
        assert_eq!(snapshot.meta.health, Health::Degraded);

        let root = proc_root.parent().unwrap().to_path_buf();
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rate_math_fails_closed_on_zero_interval_or_counter_reset() {
        assert_eq!(rate_per_second(10, 20, Duration::ZERO), None);
        assert_eq!(rate_per_second(20, 10, Duration::from_secs(1)), None);
        assert_eq!(
            rate_per_second(10, 20, Duration::from_millis(500)),
            Some(20)
        );
    }
}
