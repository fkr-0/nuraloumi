use crate::command::{CommandLimits, CommandRunner, CommandSpec};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::time::Duration;

const SNAPSHOT_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(3), 32 * 1024);
const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(12), 16 * 1024);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    pub ssid: String,
    pub signal_percent: Option<u8>,
    pub secured: bool,
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkSnapshot {
    pub meta: SnapshotMeta,
    pub radio_enabled: Option<bool>,
    pub connected: bool,
    pub interface: Option<String>,
    pub ssid: Option<String>,
    pub signal_percent: Option<u8>,
    pub networks: Vec<WifiNetwork>,
    pub issues: Vec<String>,
}

impl NetworkSnapshot {
    pub(crate) fn unavailable(timestamp: u64, issue: String) -> Self {
        Self {
            meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, "nmcli"),
            radio_enabled: None,
            connected: false,
            interface: None,
            ssid: None,
            signal_percent: None,
            networks: Vec::new(),
            issues: vec![issue],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NetworkAction {
    Radio(bool),
    ToggleRadio,
    Rescan {
        interface: Option<String>,
    },
    Connect {
        ssid: String,
        interface: Option<String>,
    },
}

pub struct NetworkProvider<R> {
    runner: R,
    timestamp_override: Option<u64>,
}

impl<R> NetworkProvider<R> {
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

impl<R: CommandRunner> Provider for NetworkProvider<R> {
    type Snapshot = NetworkSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let status_spec = CommandSpec::new("nmcli").args([
            "--terse",
            "--fields",
            "DEVICE,TYPE,STATE,CONNECTION",
            "device",
            "status",
        ]);
        let output = match self.runner.run(&status_spec, SNAPSHOT_LIMITS) {
            Ok(output) if output.status == 0 => output,
            Ok(output) => {
                return Ok(NetworkSnapshot::unavailable(
                    timestamp,
                    format!("nmcli exited with status {}", output.status),
                ));
            }
            Err(error) => {
                return Ok(NetworkSnapshot::unavailable(timestamp, error.diagnostic()));
            }
        };

        let mut connected = false;
        let mut interface = None;
        let mut connection_name = None;
        for line in output.stdout.lines() {
            let fields = split_escaped(line, ':');
            if fields.len() < 4 || fields[1] != "wifi" {
                continue;
            }
            if interface.is_none() {
                interface = Some(fields[0].clone());
            }
            if fields[2].starts_with("connected") {
                connected = true;
                interface = Some(fields[0].clone());
                if !fields[3].is_empty() && fields[3] != "--" {
                    connection_name = Some(fields[3].clone());
                }
                break;
            }
        }

        let mut issues = Vec::new();
        let radio_enabled = {
            let radio_spec = CommandSpec::new("nmcli").args(["radio", "wifi"]);
            match self.runner.run(&radio_spec, SNAPSHOT_LIMITS) {
                Ok(radio) if radio.status == 0 => {
                    match radio.stdout.trim().to_ascii_lowercase().as_str() {
                        "enabled" | "on" => Some(true),
                        "disabled" | "off" => Some(false),
                        other => {
                            issues.push(format!(
                                "nmcli returned unknown Wi-Fi radio state {other:?}"
                            ));
                            None
                        }
                    }
                }
                Ok(radio) => {
                    issues.push(format!(
                        "nmcli radio query exited with status {}",
                        radio.status
                    ));
                    None
                }
                Err(error) => {
                    issues.push(error.diagnostic());
                    None
                }
            }
        };

        let mut ssid = None;
        let mut signal_percent = None;
        let mut networks = Vec::new();
        let wifi_spec = CommandSpec::new("nmcli").args([
            "--terse",
            "--fields",
            "IN-USE,SSID,SIGNAL,SECURITY",
            "device",
            "wifi",
            "list",
            "--rescan",
            "no",
        ]);
        match self.runner.run(&wifi_spec, SNAPSHOT_LIMITS) {
            Ok(wifi) if wifi.status == 0 => {
                for line in wifi.stdout.lines().take(64) {
                    let fields = split_escaped(line, ':');
                    if fields.len() < 4 || fields[1].is_empty() {
                        continue;
                    }
                    let in_use = fields[0] == "yes" || fields[0] == "*";
                    let signal = match fields[2].parse::<u8>() {
                        Ok(value) if value <= 100 => Some(value),
                        _ => {
                            issues.push(format!(
                                "nmcli returned invalid signal strength for {:?}",
                                fields[1]
                            ));
                            None
                        }
                    };
                    let secured = !fields[3].is_empty() && fields[3] != "--";
                    if in_use {
                        ssid = Some(fields[1].clone());
                        signal_percent = signal;
                    }
                    networks.push(WifiNetwork {
                        ssid: fields[1].clone(),
                        signal_percent: signal,
                        secured,
                        connected: in_use,
                    });
                }
            }
            Ok(wifi) => issues.push(format!(
                "nmcli wifi listing exited with status {}",
                wifi.status
            )),
            Err(error) => issues.push(error.diagnostic()),
        }
        if ssid.is_none() {
            ssid = connection_name;
        }

        let health = if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        Ok(NetworkSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, "nmcli"),
            radio_enabled,
            connected,
            interface,
            ssid,
            signal_percent,
            networks,
            issues,
        })
    }
}

impl<R: CommandRunner> ActionProvider<NetworkAction> for NetworkProvider<R> {
    fn execute(&mut self, action: NetworkAction) -> Result<ActionResult, ProviderError> {
        let spec = match action {
            NetworkAction::Radio(enabled) => CommandSpec::new("nmcli").args([
                "radio".to_owned(),
                "wifi".to_owned(),
                if enabled { "on" } else { "off" }.to_owned(),
            ]),
            NetworkAction::ToggleRadio => {
                let state_spec = CommandSpec::new("nmcli").args(["radio", "wifi"]);
                let state = self.runner.run(&state_spec, SNAPSHOT_LIMITS)?;
                if state.status != 0 {
                    return Err(ProviderError::backend(format!(
                        "nmcli radio wifi exited with status {}",
                        state.status
                    )));
                }
                let enabled = match state.stdout.trim().to_ascii_lowercase().as_str() {
                    "enabled" | "on" => true,
                    "disabled" | "off" => false,
                    other => {
                        return Err(ProviderError::parse(format!(
                            "unknown nmcli Wi-Fi radio state {other:?}"
                        )));
                    }
                };
                CommandSpec::new("nmcli").args([
                    "radio".to_owned(),
                    "wifi".to_owned(),
                    if enabled { "off" } else { "on" }.to_owned(),
                ])
            }
            NetworkAction::Rescan { interface } => {
                let mut spec = CommandSpec::new("nmcli").args([
                    "device".to_owned(),
                    "wifi".to_owned(),
                    "rescan".to_owned(),
                ]);
                if let Some(interface) = interface {
                    validate_interface(&interface)?;
                    spec = spec.args(["ifname".to_owned(), interface]);
                }
                spec
            }
            NetworkAction::Connect { ssid, interface } => {
                validate_text_arg("SSID", &ssid)?;
                let mut spec = CommandSpec::new("nmcli").args([
                    "--wait".to_owned(),
                    "10".to_owned(),
                    "device".to_owned(),
                    "wifi".to_owned(),
                    "connect".to_owned(),
                    ssid,
                ]);
                if let Some(interface) = interface {
                    validate_interface(&interface)?;
                    spec = spec.args(["ifname".to_owned(), interface]);
                }
                spec
            }
        };

        let output = self.runner.run(&spec, ACTION_LIMITS)?;
        if output.status != 0 {
            return Err(ProviderError::backend(format!(
                "{} action exited with status {}",
                spec.program, output.status
            )));
        }
        Ok(ActionResult::executed("network action completed"))
    }
}

fn validate_text_arg(label: &str, value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 256
        || value.contains('\0')
        || value.contains('\n')
        || value.contains('\r')
    {
        return Err(ProviderError::backend(format!("invalid {label}")));
    }
    Ok(())
}

fn validate_interface(value: &str) -> Result<(), ProviderError> {
    validate_text_arg("interface", value)?;
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':'))
    {
        return Err(ProviderError::backend("invalid interface"));
    }
    Ok(())
}

fn split_escaped(input: &str, separator: char) -> Vec<String> {
    let mut values = vec![String::new()];
    let mut escaped = false;
    for ch in input.chars() {
        if escaped {
            values.last_mut().expect("non-empty fields").push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == separator {
            values.push(String::new());
        } else {
            values.last_mut().expect("non-empty fields").push(ch);
        }
    }
    if escaped {
        values.last_mut().expect("non-empty fields").push('\\');
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};

    fn fixture_runner() -> FixtureCommandRunner {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("nmcli").args([
                "--terse",
                "--fields",
                "DEVICE,TYPE,STATE,CONNECTION",
                "device",
                "status",
            ]),
            CommandOutput {
                status: 0,
                stdout: "wlan0:wifi:connected:Home\\:Lab\neth0:ethernet:disconnected:--\n"
                    .to_owned(),
                stderr: String::new(),
            },
        );
        runner.insert(
            CommandSpec::new("nmcli").args(["radio", "wifi"]),
            CommandOutput {
                status: 0,
                stdout: "enabled\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner.insert(
            CommandSpec::new("nmcli").args([
                "--terse",
                "--fields",
                "IN-USE,SSID,SIGNAL,SECURITY",
                "device",
                "wifi",
                "list",
                "--rescan",
                "no",
            ]),
            CommandOutput {
                status: 0,
                stdout: "yes:Home\\:Lab:82:WPA2\nno:Neighbor:20:--\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner
    }

    #[test]
    fn parses_connected_wifi_and_escaped_ssid() {
        let snapshot = NetworkProvider::new(fixture_runner())
            .with_timestamp(Some(5))
            .snapshot()
            .unwrap();
        assert!(snapshot.connected);
        assert_eq!(snapshot.interface.as_deref(), Some("wlan0"));
        assert_eq!(snapshot.ssid.as_deref(), Some("Home:Lab"));
        assert_eq!(snapshot.signal_percent, Some(82));
        assert_eq!(snapshot.radio_enabled, Some(true));
        assert_eq!(snapshot.networks.len(), 2);
        assert!(snapshot.networks[0].secured);
        assert!(!snapshot.networks[1].secured);
    }

    #[test]
    fn missing_nmcli_is_unavailable_not_fatal() {
        let snapshot = NetworkProvider::new(FixtureCommandRunner::default())
            .with_timestamp(Some(14))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.health, Health::Unavailable);
        assert!(!snapshot.connected);
    }

    #[test]
    fn toggle_radio_queries_state_before_changing_it() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("nmcli").args(["radio", "wifi"]),
            CommandOutput {
                status: 0,
                stdout: "enabled\n".to_owned(),
                stderr: String::new(),
            },
        );
        runner.insert(
            CommandSpec::new("nmcli").args(["radio", "wifi", "off"]),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        let mut provider = NetworkProvider::new(runner);
        assert!(
            provider
                .execute(NetworkAction::ToggleRadio)
                .unwrap()
                .executed
        );
    }

    #[test]
    fn connect_uses_literal_argv_for_shell_metacharacters() {
        let ssid = "Cafe;touch /tmp/never";
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("nmcli").args([
                "--wait".to_owned(),
                "10".to_owned(),
                "device".to_owned(),
                "wifi".to_owned(),
                "connect".to_owned(),
                ssid.to_owned(),
            ]),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        let mut provider = NetworkProvider::new(runner);
        let result = provider
            .execute(NetworkAction::Connect {
                ssid: ssid.to_owned(),
                interface: None,
            })
            .unwrap();
        assert!(result.executed);
    }
}
