//! Opt-in read-only SL101 evdev scanner and mapped hardware-control notifier.
//! No device grabbing, no button actions, no automatic device discovery/startup.
use nuraloumi_providers::command::{
    CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner,
};
use nuraloumi_providers::hardware_input::{
    switch_level, ButtonPressGate, EventDecoder, InputEvent, EV_KEY, EV_SW,
};
use nuraloumi_providers::slider::{SliderObserver, SliderState};
use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    os::unix::{fs::FileTypeExt, fs::OpenOptionsExt},
    path::{Path, PathBuf},
    process::ExitCode,
    thread,
    time::{Duration, Instant},
};

const HELP: &str =
    "nuraloumi-hardware-observer --device /dev/input/eventN [--seconds 60] [--max-events 1000]
    [--key volume-up=CODE] [--key volume-down=CODE]
    [--key display-on=CODE] [--key display-off=CODE]
    [--key display-toggle=CODE] [--switch CODE:OPEN_LEVEL] [--notify]
Read-only discovery is the default. Codes and switch polarity MUST be established
by physical tests. --notify requires at least one explicit mapping and sends only
NuraLoumi notifications; it does not adjust volume, power, or display state.
No grab, no autostart; duration and event count are bounded.";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum KeyRole {
    VolumeUp,
    VolumeDown,
    DisplayOn,
    DisplayOff,
    DisplayToggle,
}

impl KeyRole {
    fn parse(text: &str) -> Option<Self> {
        match text {
            "volume-up" => Some(Self::VolumeUp),
            "volume-down" => Some(Self::VolumeDown),
            "display-on" => Some(Self::DisplayOn),
            "display-off" => Some(Self::DisplayOff),
            "display-toggle" => Some(Self::DisplayToggle),
            _ => None,
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::VolumeUp => "Volume + button pressed",
            Self::VolumeDown => "Volume - button pressed",
            Self::DisplayOn => "Display on button pressed",
            Self::DisplayOff => "Display off button pressed",
            Self::DisplayToggle => "Display toggle button pressed",
        }
    }
}

#[derive(Debug)]
struct Config {
    device: PathBuf,
    seconds: u64,
    max_events: u64,
    keys: Vec<(KeyRole, u16)>,
    slider_switch: Option<(u16, u8)>,
    notify: bool,
}

fn parse_args(args: impl IntoIterator<Item = String>) -> Result<Option<Config>, String> {
    let mut args = args.into_iter();
    let mut device = None;
    let mut seconds = 60;
    let mut max_events = 1000;
    let mut keys = Vec::new();
    let mut slider_switch = None;
    let mut notify = false;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => return Ok(None),
            "--device" => {
                if device.is_some() {
                    return Err("--device may only be supplied once".into());
                }
                device = Some(PathBuf::from(args.next().ok_or("missing --device value")?));
            }
            "--seconds" => {
                seconds = args
                    .next()
                    .ok_or("missing --seconds value")?
                    .parse()
                    .map_err(|_| "--seconds needs a decimal integer")?;
            }
            "--max-events" => {
                max_events = args
                    .next()
                    .ok_or("missing --max-events value")?
                    .parse()
                    .map_err(|_| "--max-events needs a decimal integer")?;
            }
            "--key" => {
                let mapping = args.next().ok_or("missing --key value")?;
                let (role, code) = mapping.split_once('=').ok_or("--key needs ROLE=CODE")?;
                let role = KeyRole::parse(role).ok_or("unsupported --key role")?;
                let code: u16 = code.parse().map_err(|_| "--key code must be decimal u16")?;
                if keys
                    .iter()
                    .any(|(known_role, known_code)| *known_role == role || *known_code == code)
                {
                    return Err("duplicate or ambiguous --key mapping".into());
                }
                keys.push((role, code));
            }
            "--switch" => {
                if slider_switch.is_some() {
                    return Err("only one --switch mapping is supported".into());
                }
                let mapping = args.next().ok_or("missing --switch value")?;
                let (code, level) = mapping
                    .split_once(':')
                    .ok_or("--switch needs CODE:OPEN_LEVEL")?;
                let code: u16 = code
                    .parse()
                    .map_err(|_| "--switch code must be decimal u16")?;
                let level: u8 = level.parse().map_err(|_| "--switch level must be 0 or 1")?;
                if level > 1 {
                    return Err("--switch level must be 0 or 1".into());
                }
                slider_switch = Some((code, level));
            }
            "--notify" => notify = true,
            _ => return Err(format!("unknown argument: {arg}")),
        }
    }
    let device = device.ok_or("--device is required")?;
    validate_device_path(&device)?;
    if !(1..=600).contains(&seconds) {
        return Err("--seconds must be 1..600".into());
    }
    if !(1..=10_000).contains(&max_events) {
        return Err("--max-events must be 1..10000".into());
    }
    if notify && keys.is_empty() && slider_switch.is_none() {
        return Err("--notify requires a physically verified --key or --switch mapping".into());
    }
    if keys.iter().any(|(role, _)| *role == KeyRole::DisplayToggle)
        && keys
            .iter()
            .any(|(role, _)| matches!(role, KeyRole::DisplayOn | KeyRole::DisplayOff))
    {
        return Err("use display-toggle OR separate display-on/display-off roles".into());
    }
    Ok(Some(Config {
        device,
        seconds,
        max_events,
        keys,
        slider_switch,
        notify,
    }))
}

fn validate_device_path(path: &Path) -> Result<(), String> {
    if path.parent() != Some(Path::new("/dev/input")) {
        return Err("device must be a direct /dev/input/eventN path".into());
    }
    let name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or("non-UTF8 input device name")?;
    if !name
        .strip_prefix("event")
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err("device must be /dev/input/eventN (decimal N)".into());
    }
    Ok(())
}

fn open_device(path: &Path) -> Result<File, String> {
    let metadata = fs::symlink_metadata(path).map_err(|e| format!("input device metadata: {e}"))?;
    if !metadata.file_type().is_char_device() {
        return Err("input device must be a character device, not a symlink or file".into());
    }
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | libc::O_NOFOLLOW)
        .open(path)
        .map_err(|e| format!("input device open (check input group/ACL): {e}"))
}

/// Limited notification canary: no per-event retry storm if panel IPC is down.
struct Notifier {
    runner: SystemCommandRunner,
    enabled: bool,
    attempts: u8,
}

impl Notifier {
    fn new(enabled: bool) -> Self {
        Self {
            runner: SystemCommandRunner,
            enabled,
            attempts: 0,
        }
    }

    fn send(&mut self, summary: &'static str) {
        if !self.enabled {
            return;
        }
        if self.attempts >= 32 {
            eprintln!("32-notification capture budget exhausted; delivery disabled");
            self.enabled = false;
            return;
        }
        self.attempts += 1;
        let status = self.runner.run(
            &CommandSpec::new("nuraloumi-ctl").args([
                "notify",
                "--summary",
                summary,
                "--app",
                "NuraLoumi",
            ]),
            CommandLimits::new(Duration::from_secs(2), 1024),
        );
        if !matches!(status, Ok(output) if output.status == 0) {
            eprintln!("notification transport unavailable for: {summary}; delivery disabled");
            self.enabled = false;
        }
    }
}

fn report_event(
    event: InputEvent,
    config: &Config,
    gate: &mut ButtonPressGate,
    slider_level: &mut Option<SliderState>,
    notifier: &mut Notifier,
    now: Instant,
) -> bool {
    match event.kind {
        EV_KEY if (0..=2).contains(&event.value) => {
            let accepted_press = gate.accept(event, now);
            let action = match event.value {
                0 => "release",
                1 => "press",
                _ => "repeat",
            };
            println!("EV_KEY code={} action={action}", event.code);
            if accepted_press {
                if let Some((role, _)) = config.keys.iter().find(|(_, code)| *code == event.code) {
                    println!("mapped: {}", role.summary());
                    notifier.send(role.summary());
                }
            }
            true
        }
        EV_SW if event.value == 0 || event.value == 1 => {
            println!("EV_SW code={} level={}", event.code, event.value);
            if let Some((code, open_level)) = config.slider_switch {
                if let Some(level) = switch_level(event, code) {
                    *slider_level = Some(if level == open_level {
                        SliderState::Open
                    } else {
                        SliderState::Closed
                    });
                }
            }
            true
        }
        _ => false,
    }
}

fn run(config: Config) -> Result<(), String> {
    let mut file = open_device(&config.device)?;
    let mut decoder = EventDecoder::default();
    let mut gate = ButtonPressGate::new(Duration::from_millis(80));
    let mut slider = SliderObserver::new(Duration::from_millis(300));
    let mut slider_level = None;
    let mut notifier = Notifier::new(config.notify);
    let started = Instant::now();
    let mut events = 0u64;
    let mut buffer = [0u8; 4096];
    eprintln!(
        "read-only evdev capture: {} ({}s, max {} events); notifications {}",
        config.device.display(),
        config.seconds,
        config.max_events,
        if config.notify { "enabled" } else { "off" }
    );
    while started.elapsed() < Duration::from_secs(config.seconds) && events < config.max_events {
        match file.read(&mut buffer) {
            Ok(0) => return Err("input device closed; capture stopped".into()),
            Ok(read) => {
                for event in decoder.push(&buffer[..read]) {
                    if report_event(
                        event,
                        &config,
                        &mut gate,
                        &mut slider_level,
                        &mut notifier,
                        Instant::now(),
                    ) {
                        events += 1;
                        if events >= config.max_events {
                            break;
                        }
                    }
                }
                io::stdout().flush().map_err(|e| e.to_string())?;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(format!("input device read failed: {e}")),
        }
        if let Some(transition) = slider.observe(slider_level, Instant::now()) {
            println!("mapped: {}", transition.summary());
            io::stdout().flush().map_err(|e| e.to_string())?;
            notifier.send(transition.summary());
        }
        thread::sleep(Duration::from_millis(50));
    }
    eprintln!("capture ended: {events} accepted events");
    Ok(())
}

fn main() -> ExitCode {
    match parse_args(env::args().skip(1)).and_then(|config| {
        if let Some(config) = config {
            run(config)
        } else {
            println!("{HELP}");
            Ok(())
        }
    }) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}\nSee --help for usage.");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(values: &[&str]) -> Result<Option<Config>, String> {
        parse_args(values.iter().map(|v| (*v).to_owned()))
    }

    #[test]
    fn scan_is_dry_run_and_bounded() {
        let config = parse(&["--device", "/dev/input/event3"]).unwrap().unwrap();
        assert!(!config.notify);
        assert_eq!((config.seconds, config.max_events), (60, 1000));
        assert!(config.keys.is_empty());
    }

    #[test]
    fn explicit_mappings_and_notify_opt_in() {
        let config = parse(&[
            "--device",
            "/dev/input/event3",
            "--key",
            "volume-up=115",
            "--key",
            "display-toggle=116",
            "--switch",
            "1:0",
            "--notify",
            "--seconds",
            "3",
        ])
        .unwrap()
        .unwrap();
        assert_eq!(config.keys.len(), 2);
        assert_eq!(config.slider_switch, Some((1, 0)));
        assert!(config.notify);
        assert_eq!(config.seconds, 3);
    }

    #[test]
    fn refuses_ambiguous_or_unbounded_capture() {
        for values in [
            vec!["--device", "/tmp/event3"],
            vec!["--device", "/dev/input/../event3"],
            vec!["--device", "/dev/input/event3", "--notify"],
            vec!["--device", "/dev/input/event3", "--seconds", "0"],
            vec!["--device", "/dev/input/event3", "--seconds", "601"],
            vec!["--device", "/dev/input/event3", "--max-events", "0"],
            vec!["--device", "/dev/input/event3", "--switch", "1:2"],
            vec!["--device", "/dev/input/event3", "--key", "unknown=3"],
            vec![
                "--device",
                "/dev/input/event3",
                "--key",
                "display-toggle=116",
                "--key",
                "display-on=117",
            ],
            vec![
                "--device",
                "/dev/input/event3",
                "--key",
                "volume-up=115",
                "--key",
                "volume-down=115",
            ],
            vec![
                "--device",
                "/dev/input/event3",
                "--key",
                "volume-up=115",
                "--key",
                "volume-up=116",
            ],
        ] {
            assert!(parse(&values).is_err(), "accepted: {values:?}");
        }
        assert!(parse(&["--help"]).unwrap().is_none());
    }

    #[test]
    fn raw_scan_does_not_guess_a_role_or_execute_notification() {
        let config = parse(&["--device", "/dev/input/event3"]).unwrap().unwrap();
        let now = Instant::now();
        let mut gate = ButtonPressGate::new(Duration::from_millis(80));
        let mut slider_level = None;
        let mut notifier = Notifier::new(false);
        let press = InputEvent {
            kind: EV_KEY,
            code: 115,
            value: 1,
        };
        assert!(report_event(
            press,
            &config,
            &mut gate,
            &mut slider_level,
            &mut notifier,
            now
        ));
        assert!(report_event(
            InputEvent { value: 2, ..press },
            &config,
            &mut gate,
            &mut slider_level,
            &mut notifier,
            now
        ));
        assert!(report_event(
            InputEvent { value: 0, ..press },
            &config,
            &mut gate,
            &mut slider_level,
            &mut notifier,
            now
        ));
        assert!(report_event(
            press,
            &config,
            &mut gate,
            &mut slider_level,
            &mut notifier,
            now
        ));
        assert_eq!(slider_level, None);
    }

    #[test]
    fn event_only_switch_uses_timer_ticks_for_stable_state_changes() {
        let config = parse(&["--device", "/dev/input/event4", "--switch", "1:0"])
            .unwrap()
            .unwrap();
        let now = Instant::now();
        let mut gate = ButtonPressGate::new(Duration::from_millis(80));
        let mut level = None;
        let mut notifier = Notifier::new(false);
        let mut slider = SliderObserver::new(Duration::from_millis(300));
        let closed = InputEvent {
            kind: EV_SW,
            code: 1,
            value: 1,
        };
        assert!(report_event(
            closed,
            &config,
            &mut gate,
            &mut level,
            &mut notifier,
            now
        ));
        assert_eq!(level, Some(SliderState::Closed));
        assert_eq!(slider.observe(level, now), None);
        assert_eq!(
            slider.observe(level, now + Duration::from_millis(300)),
            None
        );
        let opened = InputEvent { value: 0, ..closed };
        assert!(report_event(
            opened,
            &config,
            &mut gate,
            &mut level,
            &mut notifier,
            now + Duration::from_millis(500)
        ));
        assert_eq!(level, Some(SliderState::Open));
        assert_eq!(
            slider.observe(level, now + Duration::from_millis(500)),
            None
        );
        assert_eq!(
            slider.observe(level, now + Duration::from_millis(750)),
            None
        );
        assert_eq!(
            slider.observe(level, now + Duration::from_millis(800)),
            Some(SliderState::Open)
        );
        assert_eq!(
            slider.observe(level, now + Duration::from_millis(850)),
            None
        );
    }
}
