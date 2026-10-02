use crate::command::{CommandLimits, CommandRunner, CommandSpec};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::time::Duration;

const SNAPSHOT_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(3), 16 * 1024);
const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(5), 16 * 1024);
const SINK: &str = "@DEFAULT_AUDIO_SINK@";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioSnapshot {
    pub meta: SnapshotMeta,
    pub backend: String,
    pub volume_percent: Option<u16>,
    pub muted: Option<bool>,
    pub issues: Vec<String>,
}

impl AudioSnapshot {
    pub(crate) fn unavailable(timestamp: u64, issue: String) -> Self {
        Self {
            meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, "wpctl"),
            backend: "wpctl".to_owned(),
            volume_percent: None,
            muted: None,
            issues: vec![issue],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioAction {
    SetVolume(u16),
    AdjustVolume(i16),
    SetMute(bool),
    ToggleMute,
}

pub struct AudioProvider<R> {
    runner: R,
    timestamp_override: Option<u64>,
}

impl<R> AudioProvider<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
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
}

impl<R: CommandRunner> Provider for AudioProvider<R> {
    type Snapshot = AudioSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let spec = CommandSpec::new("wpctl").args(["get-volume", SINK]);
        let output = match self.runner.run(&spec, SNAPSHOT_LIMITS) {
            Ok(output) if output.status == 0 => output,
            Ok(output) => {
                return Ok(AudioSnapshot::unavailable(
                    timestamp,
                    format!("wpctl exited with status {}", output.status),
                ));
            }
            Err(error) => return Ok(AudioSnapshot::unavailable(timestamp, error.diagnostic())),
        };

        let mut volume_percent = None;
        let mut issues = Vec::new();
        let mut tokens = output.stdout.split_whitespace();
        while let Some(token) = tokens.next() {
            if token == "Volume:" {
                if let Some(value) = tokens.next() {
                    match value.parse::<f64>() {
                        Ok(value) if value.is_finite() && value >= 0.0 => {
                            let percent = (value * 100.0).round().clamp(0.0, 1000.0);
                            volume_percent = Some(percent as u16);
                        }
                        _ => issues.push("wpctl returned invalid volume".to_owned()),
                    }
                }
                break;
            }
        }
        if volume_percent.is_none() && issues.is_empty() {
            issues.push("wpctl volume field missing".to_owned());
        }
        let muted = Some(output.stdout.contains("[MUTED]"));
        let health = if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        Ok(AudioSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, "wpctl"),
            backend: "wpctl".to_owned(),
            volume_percent,
            muted,
            issues,
        })
    }
}

impl<R: CommandRunner> ActionProvider<AudioAction> for AudioProvider<R> {
    fn execute(&mut self, action: AudioAction) -> Result<ActionResult, ProviderError> {
        let spec = match action {
            AudioAction::SetVolume(percent) => {
                if percent > 150 {
                    return Err(ProviderError::backend(
                        "volume percentage must be in 0..=150",
                    ));
                }
                CommandSpec::new("wpctl").args([
                    "set-volume".to_owned(),
                    SINK.to_owned(),
                    format!("{percent}%"),
                ])
            }
            AudioAction::AdjustVolume(delta) => {
                if delta == 0 || delta.unsigned_abs() > 100 {
                    return Err(ProviderError::backend(
                        "volume adjustment must be between -100 and 100 and non-zero",
                    ));
                }
                let suffix = if delta > 0 { '+' } else { '-' };
                CommandSpec::new("wpctl").args([
                    "set-volume".to_owned(),
                    SINK.to_owned(),
                    format!("{}%{suffix}", delta.unsigned_abs()),
                ])
            }
            AudioAction::SetMute(muted) => CommandSpec::new("wpctl").args([
                "set-mute".to_owned(),
                SINK.to_owned(),
                if muted { "1" } else { "0" }.to_owned(),
            ]),
            AudioAction::ToggleMute => CommandSpec::new("wpctl").args([
                "set-mute".to_owned(),
                SINK.to_owned(),
                "toggle".to_owned(),
            ]),
        };
        let output = self.runner.run(&spec, ACTION_LIMITS)?;
        if output.status != 0 {
            return Err(ProviderError::backend(format!(
                "wpctl action exited with status {}",
                output.status
            )));
        }
        Ok(ActionResult::executed("audio action completed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};

    #[test]
    fn missing_wpctl_is_unavailable_not_fatal() {
        let snapshot = AudioProvider::new(FixtureCommandRunner::default())
            .with_timestamp(Some(15))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.health, Health::Unavailable);
        assert_eq!(snapshot.volume_percent, None);
    }

    #[test]
    fn parses_wpctl_snapshot_and_explicit_action() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("wpctl").args(["get-volume", SINK]),
            CommandOutput {
                status: 0,
                stdout: "Volume: 0.42 [MUTED]\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner.insert(
            CommandSpec::new("wpctl").args([
                "set-volume".to_owned(),
                SINK.to_owned(),
                "50%".to_owned(),
            ]),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        let mut provider = AudioProvider::new(runner).with_timestamp(Some(11));
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.volume_percent, Some(42));
        assert_eq!(snapshot.muted, Some(true));
        assert!(
            provider
                .execute(AudioAction::SetVolume(50))
                .unwrap()
                .executed
        );
    }
}
