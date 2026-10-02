use crate::command::{CommandLimits, CommandRunner, CommandSpec};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::time::Duration;

const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(5), 16 * 1024);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSnapshot {
    pub meta: SnapshotMeta,
    pub destructive_actions_enabled: bool,
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionAction {
    Suspend,
    Reboot,
    PowerOff,
}

pub struct SessionProvider<R> {
    runner: R,
    destructive_actions_enabled: bool,
    timestamp_override: Option<u64>,
}

impl<R> SessionProvider<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            destructive_actions_enabled: false,
            timestamp_override: None,
        }
    }

    pub fn with_destructive_actions(mut self, enabled: bool) -> Self {
        self.destructive_actions_enabled = enabled;
        self
    }

    pub fn with_timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp_override = timestamp;
        self
    }
}

impl<R> Provider for SessionProvider<R> {
    type Snapshot = SessionSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        Ok(SessionSnapshot {
            meta: SnapshotMeta::new(
                timestamp_ms(self.timestamp_override),
                Health::Healthy,
                false,
                "systemd-session",
            ),
            destructive_actions_enabled: self.destructive_actions_enabled,
            issues: if self.destructive_actions_enabled {
                Vec::new()
            } else {
                vec!["destructive session actions disabled by caller capability".to_owned()]
            },
        })
    }
}

impl<R: CommandRunner> ActionProvider<SessionAction> for SessionProvider<R> {
    fn execute(&mut self, action: SessionAction) -> Result<ActionResult, ProviderError> {
        if !self.destructive_actions_enabled {
            return Ok(ActionResult::dry_run(
                "destructive session action disabled by caller capability",
            ));
        }

        let verb = match action {
            SessionAction::Suspend => "suspend",
            SessionAction::Reboot => "reboot",
            SessionAction::PowerOff => "poweroff",
        };
        let spec = CommandSpec::new("systemctl").arg(verb);
        let output = self.runner.run(&spec, ACTION_LIMITS)?;
        if output.status != 0 {
            return Err(ProviderError::backend(format!(
                "systemctl action exited with status {}",
                output.status
            )));
        }
        Ok(ActionResult::executed("session action submitted"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};

    #[test]
    fn destructive_action_is_dry_run_by_default() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("systemctl").arg("reboot"),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        let mut provider = SessionProvider::new(runner);
        let result = provider.execute(SessionAction::Reboot).unwrap();
        assert!(!result.executed);
        assert!(result.dry_run);
    }

    #[test]
    fn enabled_fixture_action_uses_exact_backend_command() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("systemctl").arg("suspend"),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        let mut provider = SessionProvider::new(runner).with_destructive_actions(true);
        assert!(provider.execute(SessionAction::Suspend).unwrap().executed);
    }
}
