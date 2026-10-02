use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::fs;
use std::path::Path;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClockSnapshot {
    pub meta: SnapshotMeta,
    pub unix_timestamp_ms: u64,
    pub time_label: String,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct ClockProvider {
    timestamp_override: Option<u64>,
    label_override: Option<String>,
}

impl ClockProvider {
    pub fn system() -> Self {
        Self::default()
    }

    pub fn fixed(timestamp: u64, label: impl Into<String>) -> Self {
        Self {
            timestamp_override: Some(timestamp),
            label_override: Some(label.into()),
        }
    }

    pub fn from_fixture(path: impl AsRef<Path>) -> Result<Self, ProviderError> {
        let value = fs::read_to_string(path)
            .map_err(|error| ProviderError::from_io("reading clock fixture", &error))?;
        let mut lines = value.lines();
        let timestamp = lines
            .next()
            .ok_or_else(|| ProviderError::parse("clock fixture missing timestamp"))?
            .trim()
            .parse::<u64>()
            .map_err(|_| ProviderError::parse("clock fixture timestamp is invalid"))?;
        let label = lines
            .next()
            .ok_or_else(|| ProviderError::parse("clock fixture missing label"))?
            .trim()
            .to_owned();
        if label.is_empty() {
            return Err(ProviderError::parse("clock fixture label is empty"));
        }
        Ok(Self::fixed(timestamp, label))
    }
}

impl Provider for ClockProvider {
    type Snapshot = ClockSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        if let Some(label) = self.label_override.clone() {
            return Ok(ClockSnapshot {
                meta: SnapshotMeta::new(timestamp, Health::Healthy, false, "fixture:clock"),
                unix_timestamp_ms: timestamp,
                time_label: label,
                issues: Vec::new(),
            });
        }

        let mut runner = SystemCommandRunner;
        let spec = CommandSpec::new("date").arg("+%H:%M");
        match runner.run(&spec, CommandLimits::new(Duration::from_secs(2), 4096)) {
            Ok(output) if output.status == 0 && !output.stdout.trim().is_empty() => {
                Ok(ClockSnapshot {
                    meta: SnapshotMeta::new(timestamp, Health::Healthy, false, "system-clock"),
                    unix_timestamp_ms: timestamp,
                    time_label: output.stdout.trim().to_owned(),
                    issues: Vec::new(),
                })
            }
            Ok(output) => Ok(ClockSnapshot {
                meta: SnapshotMeta::new(timestamp, Health::Degraded, false, "system-clock"),
                unix_timestamp_ms: timestamp,
                time_label: "--:--".to_owned(),
                issues: vec![format!("date exited with status {}", output.status)],
            }),
            Err(error) => Ok(ClockSnapshot {
                meta: SnapshotMeta::new(timestamp, Health::Degraded, false, "system-clock"),
                unix_timestamp_ms: timestamp,
                time_label: "--:--".to_owned(),
                issues: vec![error.diagnostic()],
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_clock_is_deterministic() {
        let one = ClockProvider::fixed(1_700_000_000_123, "23:13")
            .snapshot()
            .unwrap();
        let two = ClockProvider::fixed(1_700_000_000_123, "23:13")
            .snapshot()
            .unwrap();
        assert_eq!(one, two);
    }
}
