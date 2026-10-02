use crate::common::{timestamp_ms, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerSupplyKind {
    Battery,
    Adapter,
    Other,
}

impl PowerSupplyKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Battery => "battery",
            Self::Adapter => "adapter",
            Self::Other => "other",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PowerSupply {
    pub name: String,
    pub kind: PowerSupplyKind,
    pub capacity_percent: Option<u8>,
    pub status: Option<String>,
    pub charging: Option<bool>,
    pub online: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatterySnapshot {
    pub meta: SnapshotMeta,
    pub supplies: Vec<PowerSupply>,
    pub issues: Vec<String>,
}

impl BatterySnapshot {
    pub(crate) fn unavailable(timestamp: u64, source: String, issue: String) -> Self {
        Self {
            meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, source),
            supplies: Vec::new(),
            issues: vec![issue],
        }
    }
}

#[derive(Debug, Clone)]
pub struct BatteryProvider {
    root: PathBuf,
    source: String,
    timestamp_override: Option<u64>,
}

impl BatteryProvider {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            source: format!("sysfs:{}", root.display()),
            root,
            timestamp_override: None,
        }
    }

    pub fn system() -> Self {
        Self::new("/sys/class/power_supply")
    }

    pub fn with_timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp_override = timestamp;
        self
    }

    pub fn with_source(mut self, source: impl Into<String>) -> Self {
        self.source = source.into();
        self
    }
}

impl Provider for BatteryProvider {
    type Snapshot = BatterySnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let mut paths = match fs::read_dir(&self.root) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(BatterySnapshot::unavailable(
                    timestamp,
                    self.source.clone(),
                    "power_supply sysfs is absent".to_owned(),
                ));
            }
            Err(error) => {
                return Err(ProviderError::from_io("reading power_supply sysfs", &error));
            }
        };
        paths.sort();

        let mut supplies = Vec::new();
        let mut issues = Vec::new();
        for path in paths {
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                issues.push("ignored non-UTF8 power supply name".to_owned());
                continue;
            };
            let kind = match read_optional(&path.join("type"))? {
                Some(value) if value.eq_ignore_ascii_case("battery") => PowerSupplyKind::Battery,
                Some(value)
                    if value.eq_ignore_ascii_case("mains")
                        || value.eq_ignore_ascii_case("usb")
                        || value.eq_ignore_ascii_case("usb_c") =>
                {
                    PowerSupplyKind::Adapter
                }
                Some(_) | None => PowerSupplyKind::Other,
            };

            let capacity_percent = match read_optional(&path.join("capacity"))? {
                Some(value) => match value.parse::<u8>() {
                    Ok(value) if value <= 100 => Some(value),
                    Ok(_) | Err(_) => {
                        issues.push(format!("{name}: invalid capacity"));
                        None
                    }
                },
                None => None,
            };
            let status = read_optional(&path.join("status"))?;
            let charging = status.as_deref().and_then(|value| {
                if value.eq_ignore_ascii_case("charging") {
                    Some(true)
                } else if value.eq_ignore_ascii_case("discharging")
                    || value.eq_ignore_ascii_case("not charging")
                    || value.eq_ignore_ascii_case("full")
                {
                    Some(false)
                } else {
                    None
                }
            });
            let online = match read_optional(&path.join("online"))? {
                Some(value) => match value.as_str() {
                    "0" => Some(false),
                    "1" => Some(true),
                    _ => {
                        issues.push(format!("{name}: invalid online value"));
                        None
                    }
                },
                None => None,
            };

            supplies.push(PowerSupply {
                name: name.to_owned(),
                kind,
                capacity_percent,
                status,
                charging,
                online,
            });
        }

        let health = if supplies.is_empty() {
            Health::Unavailable
        } else if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        if supplies.is_empty() {
            issues.push("no power supplies discovered".to_owned());
        }

        Ok(BatterySnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, self.source.clone()),
            supplies,
            issues,
        })
    }
}

fn read_optional(path: &Path) -> Result<Option<String>, ProviderError> {
    match fs::read_to_string(path) {
        Ok(value) => Ok(Some(value.trim().to_owned())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(ProviderError::from_io("reading power_supply field", &error)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "nuraloumi-battery-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp power_supply");
        path
    }

    #[test]
    fn discovers_multiple_supplies_and_missing_fields() {
        let root = temp_root();
        let bat = root.join("BAT0");
        let ac = root.join("AC");
        fs::create_dir_all(&bat).unwrap();
        fs::create_dir_all(&ac).unwrap();
        fs::write(bat.join("type"), "Battery\n").unwrap();
        fs::write(bat.join("capacity"), "73\n").unwrap();
        fs::write(bat.join("status"), "Discharging\n").unwrap();
        fs::write(ac.join("type"), "Mains\n").unwrap();
        fs::write(ac.join("online"), "1\n").unwrap();

        let snapshot = BatteryProvider::new(&root)
            .with_timestamp(Some(7))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.timestamp_ms, 7);
        assert_eq!(snapshot.meta.health, Health::Healthy);
        assert_eq!(snapshot.supplies.len(), 2);
        assert_eq!(snapshot.supplies[0].name, "AC");
        assert_eq!(snapshot.supplies[1].capacity_percent, Some(73));
        assert_eq!(snapshot.supplies[1].charging, Some(false));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_power_supply_tree_is_unavailable_not_fatal() {
        let root = temp_root();
        fs::remove_dir_all(&root).unwrap();

        let snapshot = BatteryProvider::new(root)
            .with_timestamp(Some(12))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.health, Health::Unavailable);
        assert!(snapshot.supplies.is_empty());
    }
}
