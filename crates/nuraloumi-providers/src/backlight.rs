use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::{ProviderError, ProviderErrorCategory};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklightDevice {
    pub name: String,
    pub brightness: u64,
    pub max_brightness: u64,
    pub percent: u8,
    pub writable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklightSnapshot {
    pub meta: SnapshotMeta,
    pub devices: Vec<BacklightDevice>,
    pub issues: Vec<String>,
}

impl BacklightSnapshot {
    pub(crate) fn unavailable(timestamp: u64, source: String, issue: String) -> Self {
        Self {
            meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, source),
            devices: Vec::new(),
            issues: vec![issue],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BacklightAction {
    SetRaw { device: String, value: u64 },
    SetPercent { device: String, percent: u8 },
}

#[derive(Debug, Clone)]
pub struct BacklightProvider {
    root: PathBuf,
    source: String,
    timestamp_override: Option<u64>,
}

impl BacklightProvider {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        Self {
            source: format!("sysfs:{}", root.display()),
            root,
            timestamp_override: None,
        }
    }

    pub fn system() -> Self {
        Self::new("/sys/class/backlight")
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

impl Provider for BacklightProvider {
    type Snapshot = BacklightSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let mut paths = match fs::read_dir(&self.root) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .collect::<Vec<_>>(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(BacklightSnapshot::unavailable(
                    timestamp,
                    self.source.clone(),
                    "backlight sysfs is absent".to_owned(),
                ));
            }
            Err(error) => return Err(ProviderError::from_io("reading backlight sysfs", &error)),
        };
        paths.sort();

        let mut devices = Vec::new();
        let mut issues = Vec::new();
        for path in paths {
            let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
                issues.push("ignored non-UTF8 backlight name".to_owned());
                continue;
            };
            let brightness = match read_u64(&path.join("brightness")) {
                Ok(value) => value,
                Err(error) => {
                    issues.push(format!("{name}: {}", error.diagnostic()));
                    continue;
                }
            };
            let max_brightness = match read_u64(&path.join("max_brightness")) {
                Ok(value) if value > 0 => value,
                Ok(_) => {
                    issues.push(format!("{name}: max brightness is zero"));
                    continue;
                }
                Err(error) => {
                    issues.push(format!("{name}: {}", error.diagnostic()));
                    continue;
                }
            };
            let percent_u64 = brightness
                .saturating_mul(100)
                .saturating_add(max_brightness / 2)
                / max_brightness;
            let percent = u8::try_from(percent_u64.min(100)).unwrap_or(100);
            let writable = fs::metadata(path.join("brightness"))
                .map(|metadata| !metadata.permissions().readonly())
                .unwrap_or(false);
            devices.push(BacklightDevice {
                name: name.to_owned(),
                brightness,
                max_brightness,
                percent,
                writable,
            });
        }

        let health = if devices.is_empty() {
            Health::Unavailable
        } else if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        if devices.is_empty() {
            issues.push("no backlight devices discovered".to_owned());
        }
        Ok(BacklightSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, self.source.clone()),
            devices,
            issues,
        })
    }
}

impl ActionProvider<BacklightAction> for BacklightProvider {
    fn execute(&mut self, action: BacklightAction) -> Result<ActionResult, ProviderError> {
        let (device, raw_value) = match action {
            BacklightAction::SetRaw { device, value } => {
                let max = self.max_for(&device)?;
                if value > max {
                    return Err(ProviderError::new(
                        ProviderErrorCategory::Backend,
                        format!("brightness {value} exceeds device maximum {max}"),
                    ));
                }
                (device, value)
            }
            BacklightAction::SetPercent { device, percent } => {
                if percent > 100 {
                    return Err(ProviderError::backend(
                        "brightness percentage must be in 0..=100",
                    ));
                }
                let max = self.max_for(&device)?;
                let raw = max.saturating_mul(u64::from(percent)).saturating_add(50) / 100;
                (device, raw)
            }
        };

        validate_component(&device)?;
        let path = self.root.join(&device).join("brightness");
        fs::write(&path, raw_value.to_string()).map_err(|error| {
            if error.kind() == io::ErrorKind::PermissionDenied {
                ProviderError::permission("permission denied writing backlight brightness")
            } else {
                ProviderError::from_io("writing backlight brightness", &error)
            }
        })?;
        Ok(ActionResult::executed("backlight brightness updated"))
    }
}

impl BacklightProvider {
    fn max_for(&self, device: &str) -> Result<u64, ProviderError> {
        validate_component(device)?;
        read_u64(&self.root.join(device).join("max_brightness"))
    }
}

fn validate_component(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value == "."
        || value == ".."
        || value.contains('/')
        || value.contains('\0')
        || value.contains('\n')
        || value.contains('\r')
    {
        return Err(ProviderError::backend("invalid backlight device name"));
    }
    Ok(())
}

fn read_u64(path: &Path) -> Result<u64, ProviderError> {
    let value = fs::read_to_string(path)
        .map_err(|error| ProviderError::from_io("reading backlight field", &error))?;
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| ProviderError::parse("backlight field is not an integer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn fixture() -> (PathBuf, PathBuf) {
        let root = std::env::temp_dir().join(format!(
            "nuraloumi-backlight-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let device = root.join("panel");
        fs::create_dir_all(&device).unwrap();
        fs::write(device.join("brightness"), "50").unwrap();
        fs::write(device.join("max_brightness"), "200").unwrap();
        (root, device)
    }

    #[test]
    fn snapshot_does_not_mutate_and_action_validates_range() {
        let (root, device) = fixture();
        let before = fs::read_to_string(device.join("brightness")).unwrap();
        let mut provider = BacklightProvider::new(&root).with_timestamp(Some(9));
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.devices[0].percent, 25);
        assert_eq!(
            fs::read_to_string(device.join("brightness")).unwrap(),
            before
        );

        let error = provider
            .execute(BacklightAction::SetRaw {
                device: "panel".to_owned(),
                value: 201,
            })
            .unwrap_err();
        assert_eq!(error.category, ProviderErrorCategory::Backend);

        provider
            .execute(BacklightAction::SetPercent {
                device: "panel".to_owned(),
                percent: 50,
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(device.join("brightness")).unwrap(),
            "100"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn missing_backlight_tree_is_unavailable_not_fatal() {
        let (root, _) = fixture();
        fs::remove_dir_all(&root).unwrap();

        let snapshot = BacklightProvider::new(root)
            .with_timestamp(Some(13))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.health, Health::Unavailable);
        assert!(snapshot.devices.is_empty());
    }
}
