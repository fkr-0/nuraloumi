use crate::command::{CommandLimits, CommandRunner, CommandSpec};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

const SNAPSHOT_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(4), 64 * 1024);
const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(12), 32 * 1024);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothDevice {
    pub address: String,
    pub label: String,
    pub paired: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BluetoothSnapshot {
    pub meta: SnapshotMeta,
    pub powered: Option<bool>,
    pub devices: Vec<BluetoothDevice>,
    pub issues: Vec<String>,
}

impl BluetoothSnapshot {
    pub(crate) fn unavailable(timestamp: u64, issue: String) -> Self {
        Self {
            meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, "bluetoothctl"),
            powered: None,
            devices: Vec::new(),
            issues: vec![issue],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BluetoothAction {
    Power(bool),
    TogglePower,
    Connect { address: String },
    Disconnect { address: String },
}

pub struct BluetoothProvider<R> {
    runner: R,
    timestamp_override: Option<u64>,
}

impl<R> BluetoothProvider<R> {
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
}

impl<R: CommandRunner> Provider for BluetoothProvider<R> {
    type Snapshot = BluetoothSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let show = CommandSpec::new("bluetoothctl").arg("show");
        let show_output = match self.runner.run(&show, SNAPSHOT_LIMITS) {
            Ok(output) if output.status == 0 => output,
            Ok(output) => {
                return Ok(BluetoothSnapshot::unavailable(
                    timestamp,
                    format!("bluetoothctl show exited with status {}", output.status),
                ));
            }
            Err(error) => {
                return Ok(BluetoothSnapshot::unavailable(
                    timestamp,
                    error.diagnostic(),
                ));
            }
        };

        let powered = parse_powered(&show_output.stdout);
        let mut issues = Vec::new();
        if powered.is_none() {
            issues.push("bluetoothctl show did not report Powered".to_owned());
        }

        let all = self.device_listing(&["devices"], &mut issues);
        let paired = self.device_addresses(&["devices", "Paired"], &mut issues);
        let connected = self.device_addresses(&["devices", "Connected"], &mut issues);

        let devices = all
            .into_iter()
            .take(64)
            .map(|(address, label)| BluetoothDevice {
                paired: paired.contains(&address),
                connected: connected.contains(&address),
                address,
                label,
            })
            .collect::<Vec<_>>();

        let health = if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        Ok(BluetoothSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, "bluetoothctl"),
            powered,
            devices,
            issues,
        })
    }
}

impl<R: CommandRunner> BluetoothProvider<R> {
    fn device_listing(
        &mut self,
        args: &[&str],
        issues: &mut Vec<String>,
    ) -> BTreeMap<String, String> {
        let spec = CommandSpec::new("bluetoothctl").args(args.iter().copied());
        match self.runner.run(&spec, SNAPSHOT_LIMITS) {
            Ok(output) if output.status == 0 => parse_devices(&output.stdout),
            Ok(output) => {
                issues.push(format!(
                    "bluetoothctl {} exited with status {}",
                    args.join(" "),
                    output.status
                ));
                BTreeMap::new()
            }
            Err(error) => {
                issues.push(error.diagnostic());
                BTreeMap::new()
            }
        }
    }

    fn device_addresses(&mut self, args: &[&str], issues: &mut Vec<String>) -> BTreeSet<String> {
        self.device_listing(args, issues).into_keys().collect()
    }
}

impl<R: CommandRunner> ActionProvider<BluetoothAction> for BluetoothProvider<R> {
    fn execute(&mut self, action: BluetoothAction) -> Result<ActionResult, ProviderError> {
        let spec = match action {
            BluetoothAction::Power(enabled) => power_spec(enabled),
            BluetoothAction::TogglePower => {
                let show = CommandSpec::new("bluetoothctl").arg("show");
                let output = self.runner.run(&show, SNAPSHOT_LIMITS)?;
                if output.status != 0 {
                    return Err(ProviderError::backend(format!(
                        "bluetoothctl show exited with status {}",
                        output.status
                    )));
                }
                let powered = parse_powered(&output.stdout).ok_or_else(|| {
                    ProviderError::parse("bluetoothctl show missing Powered state")
                })?;
                power_spec(!powered)
            }
            BluetoothAction::Connect { address } => {
                validate_address(&address)?;
                CommandSpec::new("bluetoothctl").args(["connect".to_owned(), address])
            }
            BluetoothAction::Disconnect { address } => {
                validate_address(&address)?;
                CommandSpec::new("bluetoothctl").args(["disconnect".to_owned(), address])
            }
        };

        let output = self.runner.run(&spec, ACTION_LIMITS)?;
        let combined = format!("{}\n{}", output.stdout, output.stderr);
        if output.status != 0 || combined.to_ascii_lowercase().contains("failed to") {
            return Err(ProviderError::backend(format!(
                "bluetoothctl action failed with status {}",
                output.status
            )));
        }
        Ok(ActionResult::executed("bluetooth action completed"))
    }
}

fn power_spec(enabled: bool) -> CommandSpec {
    CommandSpec::new("bluetoothctl").args([
        "power".to_owned(),
        if enabled { "on" } else { "off" }.to_owned(),
    ])
}

fn parse_powered(input: &str) -> Option<bool> {
    input.lines().find_map(|line| {
        let value = line.trim().strip_prefix("Powered:")?.trim();
        match value {
            "yes" => Some(true),
            "no" => Some(false),
            _ => None,
        }
    })
}

fn parse_devices(input: &str) -> BTreeMap<String, String> {
    input
        .lines()
        .filter_map(|line| {
            let rest = line.trim().strip_prefix("Device ")?;
            let (address, label) = rest.split_once(' ')?;
            validate_address(address).ok()?;
            Some((address.to_owned(), label.trim().to_owned()))
        })
        .collect()
}

fn validate_address(value: &str) -> Result<(), ProviderError> {
    let parts = value.split(':').collect::<Vec<_>>();
    if parts.len() != 6
        || parts
            .iter()
            .any(|part| part.len() != 2 || !part.chars().all(|ch| ch.is_ascii_hexdigit()))
    {
        return Err(ProviderError::parse("invalid Bluetooth device address"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};

    fn fixture_runner() -> FixtureCommandRunner {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("bluetoothctl").arg("show"),
            CommandOutput {
                status: 0,
                stdout: "Controller 00:11:22:33:44:55\n\tPowered: yes\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner.insert(
            CommandSpec::new("bluetoothctl").arg("devices"),
            CommandOutput {
                status: 0,
                stdout: "Device AA:BB:CC:DD:EE:FF Headphones\nDevice 10:20:30:40:50:60 Phone\n"
                    .to_owned(),
                stderr: String::new(),
            },
        );
        runner.insert(
            CommandSpec::new("bluetoothctl").args(["devices", "Paired"]),
            CommandOutput {
                status: 0,
                stdout: "Device AA:BB:CC:DD:EE:FF Headphones\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner.insert(
            CommandSpec::new("bluetoothctl").args(["devices", "Connected"]),
            CommandOutput {
                status: 0,
                stdout: "Device AA:BB:CC:DD:EE:FF Headphones\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner
    }

    #[test]
    fn snapshot_parses_power_and_device_state() {
        let snapshot = BluetoothProvider::new(fixture_runner())
            .with_timestamp(Some(7))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.powered, Some(true));
        assert_eq!(snapshot.devices.len(), 2);
        let headphones = snapshot
            .devices
            .iter()
            .find(|device| device.address == "AA:BB:CC:DD:EE:FF")
            .expect("headphones");
        assert!(headphones.paired);
        assert!(headphones.connected);
    }

    #[test]
    fn missing_bluetoothctl_is_unavailable_not_fatal() {
        let snapshot = BluetoothProvider::new(FixtureCommandRunner::default())
            .with_timestamp(Some(8))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.health, Health::Unavailable);
        assert_eq!(snapshot.powered, None);
    }

    #[test]
    fn toggle_power_queries_then_changes_state() {
        let mut runner = fixture_runner();
        runner.insert(
            CommandSpec::new("bluetoothctl").args(["power", "off"]),
            CommandOutput {
                status: 0,
                stdout: "Changing power off succeeded\n".to_owned(),
                stderr: String::new(),
            },
        );
        let mut provider = BluetoothProvider::new(runner);
        assert!(
            provider
                .execute(BluetoothAction::TogglePower)
                .unwrap()
                .executed
        );
    }

    #[test]
    fn address_validation_rejects_command_like_payload() {
        let mut provider = BluetoothProvider::new(FixtureCommandRunner::default());
        let error = provider
            .execute(BluetoothAction::Connect {
                address: "AA:BB:CC:DD:EE:FF;power off".to_owned(),
            })
            .unwrap_err();
        assert_eq!(error.category, crate::ProviderErrorCategory::Parse);
    }
}
