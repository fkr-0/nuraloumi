use crate::command::{CommandLimits, CommandRunner, CommandSpec};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::thread;
use std::time::Duration;

const SNAPSHOT_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(3), 16 * 1024);
const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(5), 16 * 1024);
const READBACK_LIMITS: CommandLimits = CommandLimits::new(Duration::from_millis(400), 1024);
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

impl<R: CommandRunner> AudioProvider<R> {
    /// Per-observation deadline prevents ordinary 3s snapshots multiplying
    /// into a long acknowledgement delay.
    fn mute_readback(&mut self) -> Result<Option<bool>, ProviderError> {
        let output = self.runner.run(
            &CommandSpec::new("wpctl").args(["get-volume", SINK]),
            READBACK_LIMITS,
        )?;
        if output.status != 0 {
            return Err(ProviderError::backend(format!(
                "wpctl readback exited with status {}",
                output.status
            )));
        }
        let mut tokens = output.stdout.split_whitespace();
        while let Some(token) = tokens.next() {
            if token == "Volume:" {
                return Ok(tokens
                    .next()
                    .and_then(|value| value.parse::<f64>().ok())
                    .filter(|value| value.is_finite() && *value >= 0.0)
                    .map(|_| output.stdout.contains("[MUTED]")));
            }
        }
        Ok(None)
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
        let muted = volume_percent.map(|_| output.stdout.contains("[MUTED]"));
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
        // An explicit mute request has a measurable postcondition. In particular,
        // volume == 0 does NOT imply mute. Capture toggle's initial state before
        // issuing the action, so stale or out-of-order replies cannot be accepted.
        let expected_mute = match &action {
            AudioAction::SetMute(value) => Some(*value),
            AudioAction::ToggleMute => Some(!self.mute_readback()?.ok_or_else(|| {
                ProviderError::backend("cannot toggle mute without a valid current mute state")
            })?),
            _ => None,
        };
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
        if let Some(expected) = expected_mute {
            // Only acknowledge an observed state, not a successful command exit.
            // The short bounded retries accommodate asynchronous PipeWire changes.
            for attempt in 0..4 {
                if self.mute_readback()? == Some(expected) {
                    return Ok(ActionResult::executed("audio mute state verified"));
                }
                if attempt != 3 {
                    thread::sleep(Duration::from_millis(40));
                }
            }
            return Err(ProviderError::backend(
                "wpctl accepted mute command but readback did not confirm requested state",
            ));
        }
        Ok(ActionResult::executed("audio action completed"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};
    use std::collections::VecDeque;

    struct ScriptedRunner {
        steps: VecDeque<(CommandSpec, Result<CommandOutput, ProviderError>)>,
    }
    impl CommandRunner for ScriptedRunner {
        fn run(
            &mut self,
            spec: &CommandSpec,
            _limits: CommandLimits,
        ) -> Result<CommandOutput, ProviderError> {
            let (expected, result) = self.steps.pop_front().expect("unexpected backend call");
            assert_eq!(&expected, spec, "out-of-order backend command");
            result
        }
    }
    fn get() -> CommandSpec {
        CommandSpec::new("wpctl").args(["get-volume", SINK])
    }
    fn mute(on: bool) -> CommandSpec {
        CommandSpec::new("wpctl").args(["set-mute", SINK, if on { "1" } else { "0" }])
    }
    fn ok(output: &str) -> Result<CommandOutput, ProviderError> {
        Ok(CommandOutput {
            status: 0,
            stdout: output.into(),
            stderr: String::new(),
        })
    }
    fn scripted(
        steps: Vec<(CommandSpec, Result<CommandOutput, ProviderError>)>,
    ) -> AudioProvider<ScriptedRunner> {
        AudioProvider::new(ScriptedRunner {
            steps: steps.into(),
        })
    }

    #[test]
    fn mute_waits_for_delayed_readback_and_preserves_volume() {
        let mut provider = scripted(vec![
            (mute(true), ok("")),
            (get(), ok("Volume: 0.35\n")),
            (get(), ok("Volume: 0.35 [MUTED]\n")),
        ]);
        assert!(
            provider
                .execute(AudioAction::SetMute(true))
                .unwrap()
                .executed
        );
    }

    #[test]
    fn mute_rejects_stale_readback() {
        let mut steps = vec![(mute(true), ok(""))];
        for _ in 0..4 {
            steps.push((get(), ok("Volume: 0.35\n")));
        }
        let error = scripted(steps)
            .execute(AudioAction::SetMute(true))
            .unwrap_err();
        assert!(error.diagnostic().contains("readback"));
    }

    #[test]
    fn mute_rejects_backend_failure() {
        let error = scripted(vec![(
            mute(true),
            Ok(CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: "failure".into(),
            }),
        )])
        .execute(AudioAction::SetMute(true))
        .unwrap_err();
        assert!(error.diagnostic().contains("status 1"));
    }

    #[test]
    fn zero_volume_is_not_muted() {
        let mut provider = scripted(vec![(get(), ok("Volume: 0.00\n"))]);
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.volume_percent, Some(0));
        assert_eq!(snapshot.muted, Some(false));
    }

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
