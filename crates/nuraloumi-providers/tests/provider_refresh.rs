//! Exact-argv, no-device fixture coverage of provider refresh/recovery.

use nuraloumi_providers::audio::AudioAction;
use nuraloumi_providers::command::{CommandLimits, CommandOutput, CommandRunner, CommandSpec};
use nuraloumi_providers::common::{ActionProvider, Health};
use nuraloumi_providers::refresh::{CachedProvider, RefreshPolicy};
use nuraloumi_providers::{AudioProvider, BluetoothProvider, NetworkProvider, ProviderError};
use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

struct SequenceRunner {
    steps: VecDeque<(CommandSpec, CommandOutput)>,
    calls: Rc<Cell<usize>>,
}

impl SequenceRunner {
    fn new(steps: Vec<(CommandSpec, CommandOutput)>, calls: Rc<Cell<usize>>) -> Self {
        Self {
            steps: steps.into(),
            calls,
        }
    }
}

impl CommandRunner for SequenceRunner {
    fn run(
        &mut self,
        spec: &CommandSpec,
        limits: CommandLimits,
    ) -> Result<CommandOutput, ProviderError> {
        assert!(
            limits.timeout <= Duration::from_secs(12),
            "backend deadline exceeded"
        );
        assert!(
            limits.max_output_bytes <= 64 * 1024,
            "backend output cap exceeded"
        );
        let (expected, response) = self
            .steps
            .pop_front()
            .expect("unexpected extra backend call");
        assert_eq!(&expected, spec, "backend used unexpected arguments");
        self.calls.set(self.calls.get() + 1);
        Ok(response)
    }
}

fn ok(text: &str) -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: text.to_owned(),
        stderr: String::new(),
    }
}

fn failed() -> CommandOutput {
    CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: "service down".to_owned(),
    }
}

fn policy() -> RefreshPolicy {
    RefreshPolicy::new(
        Duration::from_secs(5),
        Duration::from_secs(3),
        Duration::from_secs(6),
    )
}

fn network_status() -> CommandSpec {
    CommandSpec::new("nmcli").args([
        "--terse",
        "--fields",
        "DEVICE,TYPE,STATE,CONNECTION",
        "device",
        "status",
    ])
}

fn network_radio() -> CommandSpec {
    CommandSpec::new("nmcli").args(["radio", "wifi"])
}

fn network_wifi() -> CommandSpec {
    CommandSpec::new("nmcli").args([
        "--terse",
        "--fields",
        "IN-USE,SSID,SIGNAL,SECURITY",
        "device",
        "wifi",
        "list",
        "--rescan",
        "no",
    ])
}

#[test]
fn network_refresh_preserves_recent_last_good_and_recovers_after_nmcli_failure() {
    let calls = Rc::new(Cell::new(0));
    let runner = SequenceRunner::new(
        vec![
            (network_status(), ok("wlan0:wifi:connected:Home\\:Lab\n")),
            (network_radio(), ok("enabled\n")),
            (network_wifi(), ok("yes:Home\\:Lab:77:WPA2\n")),
            (network_status(), failed()),
            (network_status(), ok("wlan0:wifi:disconnected:--\n")),
            (network_radio(), ok("enabled\n")),
            (network_wifi(), ok("no:Neighbor:21:--\n")),
        ],
        calls.clone(),
    );
    let provider = NetworkProvider::new(runner).with_timestamp(Some(42));
    let mut cache = CachedProvider::new(provider, policy());
    assert_eq!(calls.get(), 0, "construction must not execute a command");

    let first = cache.snapshot_at(0).unwrap();
    assert!(first.connected);
    assert_eq!(first.ssid.as_deref(), Some("Home:Lab"));
    assert_eq!(calls.get(), 3);
    assert_eq!(cache.snapshot_at(1_000).unwrap(), first);
    assert_eq!(
        calls.get(),
        3,
        "idle snapshot must not repoll before deadline"
    );

    let stale = cache.snapshot_at(5_000).unwrap();
    assert_eq!(calls.get(), 4);
    assert!(stale.connected);
    assert_eq!(
        stale.meta.timestamp_ms, 42,
        "freshness must not be falsified"
    );
    assert_eq!(stale.meta.health, Health::Degraded);
    assert!(stale.meta.stale);
    assert!(stale
        .issues
        .iter()
        .any(|issue| issue.contains("refresh failed")));
    assert!(cache.snapshot_at(6_000).unwrap().meta.stale);
    assert_eq!(calls.get(), 4, "failure retries must be rate-limited");

    let recovered = cache.snapshot_at(8_000).unwrap();
    assert!(!recovered.connected);
    assert!(!recovered.meta.stale);
    assert_eq!(recovered.meta.health, Health::Healthy);
    assert_eq!(calls.get(), 7);
}

#[test]
fn audio_stale_expiry_recovery_and_successful_action_invalidation() {
    let calls = Rc::new(Cell::new(0));
    let volume = CommandSpec::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]);
    let mute = CommandSpec::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", "1"]);
    let runner = SequenceRunner::new(
        vec![
            (volume.clone(), ok("Volume: 0.75 [MUTED]\n")),
            (volume.clone(), failed()),
            (volume.clone(), failed()),
            (volume.clone(), ok("Volume: 0.14\n")),
            (mute, ok("")),
            (volume, ok("Volume: 0.14 [MUTED]\n")),
        ],
        calls.clone(),
    );
    let provider = AudioProvider::new(runner).with_timestamp(Some(64));
    let mut cache = CachedProvider::new(provider, policy());

    assert_eq!(cache.snapshot_at(0).unwrap().volume_percent, Some(75));
    let stale = cache.snapshot_at(5_000).unwrap();
    assert_eq!(stale.volume_percent, Some(75));
    assert!(stale.meta.stale);
    // The retry happens at 8s and max-stale of 6s has expired.
    let expired = cache.snapshot_at(8_000).unwrap();
    assert_eq!(expired.meta.health, Health::Unavailable);
    assert_eq!(expired.volume_percent, None);
    assert!(!expired.meta.stale);
    assert_eq!(calls.get(), 3);

    let recovered = cache.snapshot_at(11_000).unwrap();
    assert_eq!(recovered.meta.health, Health::Healthy);
    assert_eq!(recovered.volume_percent, Some(14));
    assert!(!recovered.meta.stale);
    assert!(cache.execute(AudioAction::SetMute(true)).unwrap().executed);
    // A mutation forces a fresh read, even inside the normal 5s polling window.
    let refreshed = cache.snapshot_at(11_001).unwrap();
    assert_eq!(refreshed.volume_percent, Some(14));
    assert_eq!(refreshed.muted, Some(true));
    assert_eq!(calls.get(), 6);
}

#[test]
fn failed_mutation_invalidates_cached_state_even_if_backend_reports_error() {
    let calls = Rc::new(Cell::new(0));
    let volume = CommandSpec::new("wpctl")
        .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
        .clone();
    let mute = CommandSpec::new("wpctl").args(["set-mute", "@DEFAULT_AUDIO_SINK@", "1"]);
    let runner = SequenceRunner::new(
        vec![
            (volume.clone(), ok("Volume: 0.75\n")),
            (mute, failed()),
            (volume, ok("Volume: 0.22\n")),
        ],
        calls.clone(),
    );
    let mut cache = CachedProvider::new(AudioProvider::new(runner), policy());
    assert_eq!(cache.snapshot_at(0).unwrap().volume_percent, Some(75));
    assert!(cache.execute(AudioAction::SetMute(true)).is_err());
    assert_eq!(cache.snapshot_at(1).unwrap().volume_percent, Some(22));
    assert_eq!(calls.get(), 3);
}

fn bluetooth_show() -> CommandSpec {
    CommandSpec::new("bluetoothctl").arg("show")
}

fn bluetooth_devices(args: &[&str]) -> CommandSpec {
    CommandSpec::new("bluetoothctl").args(args.iter().copied())
}

#[test]
fn bluetooth_disappearance_returns_degraded_devices_then_recovers_empty() {
    let calls = Rc::new(Cell::new(0));
    let runner = SequenceRunner::new(
        vec![
            (
                bluetooth_show(),
                ok("Controller 00:11:22:33:44:55\n Powered: yes\n"),
            ),
            (
                bluetooth_devices(&["devices"]),
                ok("Device AA:BB:CC:DD:EE:FF Headphones\n"),
            ),
            (
                bluetooth_devices(&["devices", "Paired"]),
                ok("Device AA:BB:CC:DD:EE:FF Headphones\n"),
            ),
            (
                bluetooth_devices(&["devices", "Connected"]),
                ok("Device AA:BB:CC:DD:EE:FF Headphones\n"),
            ),
            (bluetooth_show(), failed()),
            (
                bluetooth_show(),
                ok("Controller 00:11:22:33:44:55\n Powered: no\n"),
            ),
            (bluetooth_devices(&["devices"]), ok("")),
            (bluetooth_devices(&["devices", "Paired"]), ok("")),
            (bluetooth_devices(&["devices", "Connected"]), ok("")),
        ],
        calls.clone(),
    );
    let provider = BluetoothProvider::new(runner).with_timestamp(Some(13));
    let mut cache = CachedProvider::new(provider, policy());

    let initial = cache.snapshot_at(0).unwrap();
    assert_eq!(initial.devices.len(), 1);
    assert!(initial.devices[0].paired);
    assert!(initial.devices[0].connected);
    let stale = cache.snapshot_at(5_000).unwrap();
    assert_eq!(stale.devices.len(), 1);
    assert!(stale.meta.stale);
    assert_eq!(stale.meta.health, Health::Degraded);
    assert_eq!(calls.get(), 5);
    let recovered = cache.snapshot_at(8_000).unwrap();
    assert_eq!(recovered.powered, Some(false));
    assert!(recovered.devices.is_empty());
    assert_eq!(recovered.meta.health, Health::Healthy);
    assert!(!recovered.meta.stale);
    assert_eq!(calls.get(), 9);
}

#[test]
fn initial_absent_service_is_negatively_cached_without_spinning() {
    let calls = Rc::new(Cell::new(0));
    let volume = CommandSpec::new("wpctl").args(["get-volume", "@DEFAULT_AUDIO_SINK@"]);
    let runner = SequenceRunner::new(
        vec![(volume.clone(), failed()), (volume, ok("Volume: 0.21\n"))],
        calls.clone(),
    );
    let mut cache = CachedProvider::new(AudioProvider::new(runner), policy());

    assert_eq!(
        cache.snapshot_at(100).unwrap().meta.health,
        Health::Unavailable
    );
    assert_eq!(
        cache.snapshot_at(1_100).unwrap().meta.health,
        Health::Unavailable
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(cache.snapshot_at(3_100).unwrap().volume_percent, Some(21));
    assert_eq!(calls.get(), 2);
}
