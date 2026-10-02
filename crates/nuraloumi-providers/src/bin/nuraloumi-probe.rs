use nuraloumi_providers::probe::action_result_json;
use nuraloumi_providers::{
    ActionProvider, AudioAction, AudioProvider, BacklightAction, BacklightProvider,
    BluetoothAction, BluetoothProvider, CommandRunner, FixtureCommandRunner, NetworkAction,
    NetworkProvider, ProbeSnapshot, ProviderError, SessionAction, SessionProvider,
    SystemCommandRunner,
};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<(), String> {
    let (args, fixture, destructive_enabled) = parse_globals(std::env::args().skip(1).collect())?;
    if args.is_empty() || args == ["snapshot"] {
        let snapshot = match fixture {
            Some(root) => ProbeSnapshot::fixture(root),
            None => ProbeSnapshot::live(),
        };
        print!("{}", snapshot.to_json_pretty());
        return Ok(());
    }
    if args == ["--help"] || args == ["help"] {
        print_help();
        return Ok(());
    }
    if args.first().map(String::as_str) != Some("action") {
        return Err("expected 'snapshot' or explicit 'action' subcommand".to_owned());
    }

    let result = execute_action(&args[1..], fixture.as_deref(), destructive_enabled)
        .map_err(|error| format!("provider action failed: {error}"))?;
    print!("{}", action_result_json(&result));
    Ok(())
}

fn parse_globals(args: Vec<String>) -> Result<(Vec<String>, Option<PathBuf>, bool), String> {
    let mut retained = Vec::new();
    let mut fixture = None;
    let mut destructive_enabled = false;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--fixture" => {
                let value = args
                    .get(index + 1)
                    .ok_or_else(|| "--fixture requires a directory".to_owned())?;
                fixture = Some(PathBuf::from(value));
                index += 2;
            }
            "--enable-destructive" => {
                destructive_enabled = true;
                index += 1;
            }
            other => {
                retained.push(other.to_owned());
                index += 1;
            }
        }
    }
    Ok((retained, fixture, destructive_enabled))
}

fn command_runner(fixture: Option<&Path>) -> Result<Box<dyn CommandRunner>, ProviderError> {
    match fixture {
        Some(root) => Ok(Box::new(FixtureCommandRunner::from_dir(
            root.join("commands"),
        )?)),
        None => Ok(Box::new(SystemCommandRunner)),
    }
}

fn execute_action(
    args: &[String],
    fixture: Option<&Path>,
    destructive_enabled: bool,
) -> Result<nuraloumi_providers::ActionResult, ProviderError> {
    match args {
        [provider, operation, device, value]
            if provider == "backlight" && operation == "set-percent" =>
        {
            let percent = value
                .parse::<u8>()
                .map_err(|_| ProviderError::parse("brightness percent must be an integer"))?;
            let root = fixture
                .map(|path| path.join("sys/class/backlight"))
                .unwrap_or_else(|| PathBuf::from("/sys/class/backlight"));
            BacklightProvider::new(root).execute(BacklightAction::SetPercent {
                device: device.clone(),
                percent,
            })
        }
        [provider, operation, device, value]
            if provider == "backlight" && operation == "set-raw" =>
        {
            let raw = value
                .parse::<u64>()
                .map_err(|_| ProviderError::parse("brightness value must be an integer"))?;
            let root = fixture
                .map(|path| path.join("sys/class/backlight"))
                .unwrap_or_else(|| PathBuf::from("/sys/class/backlight"));
            BacklightProvider::new(root).execute(BacklightAction::SetRaw {
                device: device.clone(),
                value: raw,
            })
        }
        [provider, operation, state] if provider == "network" && operation == "radio" => {
            let enabled = parse_on_off(state)?;
            NetworkProvider::new(command_runner(fixture)?).execute(NetworkAction::Radio(enabled))
        }
        [provider, operation] if provider == "network" && operation == "toggle-radio" => {
            NetworkProvider::new(command_runner(fixture)?).execute(NetworkAction::ToggleRadio)
        }
        [provider, operation] if provider == "network" && operation == "rescan" => {
            NetworkProvider::new(command_runner(fixture)?)
                .execute(NetworkAction::Rescan { interface: None })
        }
        [provider, operation, interface] if provider == "network" && operation == "rescan" => {
            NetworkProvider::new(command_runner(fixture)?).execute(NetworkAction::Rescan {
                interface: Some(interface.clone()),
            })
        }
        [provider, operation, ssid] if provider == "network" && operation == "connect" => {
            NetworkProvider::new(command_runner(fixture)?).execute(NetworkAction::Connect {
                ssid: ssid.clone(),
                interface: None,
            })
        }
        [provider, operation, ssid, interface]
            if provider == "network" && operation == "connect" =>
        {
            NetworkProvider::new(command_runner(fixture)?).execute(NetworkAction::Connect {
                ssid: ssid.clone(),
                interface: Some(interface.clone()),
            })
        }
        [provider, operation, state] if provider == "bluetooth" && operation == "power" => {
            let enabled = parse_on_off(state)?;
            BluetoothProvider::new(command_runner(fixture)?)
                .execute(BluetoothAction::Power(enabled))
        }
        [provider, operation] if provider == "bluetooth" && operation == "toggle-power" => {
            BluetoothProvider::new(command_runner(fixture)?).execute(BluetoothAction::TogglePower)
        }
        [provider, operation, address] if provider == "bluetooth" && operation == "connect" => {
            BluetoothProvider::new(command_runner(fixture)?).execute(BluetoothAction::Connect {
                address: address.clone(),
            })
        }
        [provider, operation, address] if provider == "bluetooth" && operation == "disconnect" => {
            BluetoothProvider::new(command_runner(fixture)?).execute(BluetoothAction::Disconnect {
                address: address.clone(),
            })
        }
        [provider, operation, value] if provider == "audio" && operation == "set" => {
            let value = value
                .parse::<u16>()
                .map_err(|_| ProviderError::parse("volume must be an integer percentage"))?;
            AudioProvider::new(command_runner(fixture)?).execute(AudioAction::SetVolume(value))
        }
        [provider, operation, value] if provider == "audio" && operation == "adjust" => {
            let value = value
                .parse::<i16>()
                .map_err(|_| ProviderError::parse("volume delta must be an integer"))?;
            AudioProvider::new(command_runner(fixture)?).execute(AudioAction::AdjustVolume(value))
        }
        [provider, operation, state] if provider == "audio" && operation == "mute" => {
            let action = match state.as_str() {
                "on" => AudioAction::SetMute(true),
                "off" => AudioAction::SetMute(false),
                "toggle" => AudioAction::ToggleMute,
                _ => {
                    return Err(ProviderError::parse(
                        "mute state must be on, off, or toggle",
                    ))
                }
            };
            AudioProvider::new(command_runner(fixture)?).execute(action)
        }
        [provider, operation] if provider == "session" => {
            let action = match operation.as_str() {
                "suspend" => SessionAction::Suspend,
                "reboot" => SessionAction::Reboot,
                "poweroff" => SessionAction::PowerOff,
                _ => {
                    return Err(ProviderError::parse(
                        "session action must be suspend, reboot, or poweroff",
                    ));
                }
            };
            SessionProvider::new(command_runner(fixture)?)
                .with_destructive_actions(destructive_enabled)
                .execute(action)
        }
        _ => Err(ProviderError::parse(
            "invalid action; run nuraloumi-probe --help for supported forms",
        )),
    }
}

fn parse_on_off(value: &str) -> Result<bool, ProviderError> {
    match value {
        "on" => Ok(true),
        "off" => Ok(false),
        _ => Err(ProviderError::parse("state must be on or off")),
    }
}

fn print_help() {
    println!(
        "nuraloumi-probe [--fixture DIR] [snapshot]\n\
         nuraloumi-probe [--fixture DIR] action backlight set-percent DEVICE PERCENT\n\
         nuraloumi-probe [--fixture DIR] action backlight set-raw DEVICE VALUE\n\
         nuraloumi-probe [--fixture DIR] action network radio on|off\n\
         nuraloumi-probe [--fixture DIR] action network toggle-radio\n\
         nuraloumi-probe [--fixture DIR] action network rescan [INTERFACE]\n\
         nuraloumi-probe [--fixture DIR] action network connect SSID [INTERFACE]\n\
         nuraloumi-probe [--fixture DIR] action bluetooth power on|off\n\
         nuraloumi-probe [--fixture DIR] action bluetooth toggle-power\n\
         nuraloumi-probe [--fixture DIR] action bluetooth connect|disconnect ADDRESS\n\
         nuraloumi-probe [--fixture DIR] action audio set PERCENT\n\
         nuraloumi-probe [--fixture DIR] action audio adjust DELTA\n\
         nuraloumi-probe [--fixture DIR] action audio mute on|off|toggle\n\
         nuraloumi-probe [--fixture DIR] [--enable-destructive] action session suspend|reboot|poweroff\n\n\
         Session actions are dry-run/disabled unless --enable-destructive is supplied.\n\
         Network connect intentionally accepts no password in Wave 1; credential provisioning stays external."
    );
}
