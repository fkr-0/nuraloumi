use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::{ProviderError, ProviderErrorCategory};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklightDevice {
    pub name: String,
    pub brightness: u64,
    /// Minimum exposed by the driver, or zero if no minimum is exposed.
    pub min_brightness: u64,
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
            let min_brightness = match read_optional_min(&path) {
                Ok(min) if min < max_brightness => min,
                Ok(_) => {
                    issues.push(format!("{name}: minimum is not below maximum"));
                    continue;
                }
                Err(error) => {
                    issues.push(format!("{name}: {}", error.diagnostic()));
                    continue;
                }
            };
            let range = max_brightness - min_brightness;
            let percent_u64 = brightness
                .clamp(min_brightness, max_brightness)
                .saturating_sub(min_brightness)
                .saturating_mul(100)
                .saturating_add(range / 2)
                / range;
            let percent = u8::try_from(percent_u64.min(100)).unwrap_or(100);
            let writable = fs::metadata(path.join("brightness"))
                .map(|metadata| !metadata.permissions().readonly())
                .unwrap_or(false);
            devices.push(BacklightDevice {
                name: name.to_owned(),
                brightness,
                min_brightness,
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
                let (min, max) = self.bounds_for(&device)?;
                if value < min || value > max {
                    return Err(ProviderError::new(
                        ProviderErrorCategory::Backend,
                        format!("brightness {value} outside device range {min}..={max}"),
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
                let (min, max) = self.bounds_for(&device)?;
                let range = max - min;
                let raw = min + range.saturating_mul(u64::from(percent)).saturating_add(50) / 100;
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
        // A successful write alone is not proof the driver applied the value.
        let actual = read_u64(&path)?;
        if actual != raw_value {
            return Err(ProviderError::backend(format!(
                "backlight readback {actual} differs from requested {raw_value}"
            )));
        }
        Ok(ActionResult::executed("backlight brightness updated"))
    }
}

impl BacklightProvider {
    fn bounds_for(&self, device: &str) -> Result<(u64, u64), ProviderError> {
        validate_component(device)?;
        let path = self.root.join(device);
        let max = read_u64(&path.join("max_brightness"))?;
        let min = read_optional_min(&path)?;
        if max == 0 || min >= max {
            return Err(ProviderError::backend("invalid backlight minimum/maximum"));
        }
        Ok((min, max))
    }
}

fn read_optional_min(device_path: &Path) -> Result<u64, ProviderError> {
    match fs::read_to_string(device_path.join("min_brightness")) {
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .map_err(|_| ProviderError::parse("backlight minimum is not an integer")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(ProviderError::from_io("reading backlight minimum", &error)),
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
    #[test]
    fn nonzero_minimum_is_normalized_and_percent_set_respects_bounds() {
        let (root, device) = fixture();
        fs::write(device.join("min_brightness"), "20").unwrap();
        fs::write(device.join("brightness"), "20").unwrap();
        let mut provider = BacklightProvider::new(&root);
        assert_eq!(provider.snapshot().unwrap().devices[0].percent, 0);
        provider
            .execute(BacklightAction::SetPercent {
                device: "panel".into(),
                percent: 50,
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(device.join("brightness")).unwrap(),
            "110"
        );
        provider
            .execute(BacklightAction::SetPercent {
                device: "panel".into(),
                percent: 100,
            })
            .unwrap();
        assert_eq!(
            fs::read_to_string(device.join("brightness")).unwrap(),
            "200"
        );
        fs::remove_dir_all(root).unwrap();
    }
}
