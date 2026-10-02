use crate::audio::{AudioProvider, AudioSnapshot};
use crate::backlight::{BacklightProvider, BacklightSnapshot};
use crate::battery::{BatteryProvider, BatterySnapshot};
use crate::bluetooth::{BluetoothProvider, BluetoothSnapshot};
use crate::clock::{ClockProvider, ClockSnapshot};
use crate::command::{FixtureCommandRunner, SystemCommandRunner};
use crate::common::{ActionResult, Health, Provider, SnapshotMeta};
use crate::network::{NetworkProvider, NetworkSnapshot};
use crate::session::{SessionProvider, SessionSnapshot};
use std::fmt::Write as _;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeSnapshot {
    pub battery: BatterySnapshot,
    pub backlight: BacklightSnapshot,
    pub network: NetworkSnapshot,
    pub bluetooth: BluetoothSnapshot,
    pub audio: AudioSnapshot,
    pub clock: ClockSnapshot,
    pub session: SessionSnapshot,
}

impl ProbeSnapshot {
    pub fn live() -> Self {
        let mut clock_provider = ClockProvider::system();
        let clock = clock_provider
            .snapshot()
            .unwrap_or_else(|error| ClockSnapshot {
                meta: SnapshotMeta::new(0, Health::Unavailable, false, "system-clock"),
                unix_timestamp_ms: 0,
                time_label: "--:--".to_owned(),
                issues: vec![error.diagnostic()],
            });
        let timestamp = clock.unix_timestamp_ms;

        let mut battery_provider = BatteryProvider::system().with_timestamp(Some(timestamp));
        let battery = battery_provider.snapshot().unwrap_or_else(|error| {
            BatterySnapshot::unavailable(
                timestamp,
                "sysfs:/sys/class/power_supply".to_owned(),
                error.diagnostic(),
            )
        });

        let mut backlight_provider = BacklightProvider::system().with_timestamp(Some(timestamp));
        let backlight = backlight_provider.snapshot().unwrap_or_else(|error| {
            BacklightSnapshot::unavailable(
                timestamp,
                "sysfs:/sys/class/backlight".to_owned(),
                error.diagnostic(),
            )
        });

        let mut network_provider =
            NetworkProvider::new(SystemCommandRunner).with_timestamp(Some(timestamp));
        let network = network_provider
            .snapshot()
            .unwrap_or_else(|error| NetworkSnapshot::unavailable(timestamp, error.diagnostic()));

        let mut bluetooth_provider =
            BluetoothProvider::new(SystemCommandRunner).with_timestamp(Some(timestamp));
        let bluetooth = bluetooth_provider
            .snapshot()
            .unwrap_or_else(|error| BluetoothSnapshot::unavailable(timestamp, error.diagnostic()));

        let mut audio_provider =
            AudioProvider::new(SystemCommandRunner).with_timestamp(Some(timestamp));
        let audio = audio_provider
            .snapshot()
            .unwrap_or_else(|error| AudioSnapshot::unavailable(timestamp, error.diagnostic()));

        let mut session_provider =
            SessionProvider::new(SystemCommandRunner).with_timestamp(Some(timestamp));
        let session = session_provider
            .snapshot()
            .unwrap_or_else(|error| SessionSnapshot {
                meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, "systemd-session"),
                destructive_actions_enabled: false,
                issues: vec![error.diagnostic()],
            });

        Self {
            battery,
            backlight,
            network,
            bluetooth,
            audio,
            clock,
            session,
        }
    }

    pub fn fixture(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref();
        let mut clock_provider = ClockProvider::from_fixture(root.join("clock.txt"))
            .unwrap_or_else(|_| ClockProvider::fixed(0, "00:00"));
        let mut clock = clock_provider
            .snapshot()
            .unwrap_or_else(|error| ClockSnapshot {
                meta: SnapshotMeta::new(0, Health::Unavailable, false, "fixture:clock"),
                unix_timestamp_ms: 0,
                time_label: "00:00".to_owned(),
                issues: vec![error.diagnostic()],
            });
        if clock.unix_timestamp_ms == 0 && clock.issues.is_empty() {
            clock.meta.health = Health::Degraded;
            clock
                .issues
                .push("clock fixture missing or invalid; using deterministic fallback".to_owned());
        }
        let timestamp = clock.unix_timestamp_ms;

        let mut battery_provider = BatteryProvider::new(root.join("sys/class/power_supply"))
            .with_source("fixture:power_supply")
            .with_timestamp(Some(timestamp));
        let battery = battery_provider.snapshot().unwrap_or_else(|error| {
            BatterySnapshot::unavailable(
                timestamp,
                "fixture:power_supply".to_owned(),
                error.diagnostic(),
            )
        });

        let mut backlight_provider = BacklightProvider::new(root.join("sys/class/backlight"))
            .with_source("fixture:backlight")
            .with_timestamp(Some(timestamp));
        let backlight = backlight_provider.snapshot().unwrap_or_else(|error| {
            BacklightSnapshot::unavailable(
                timestamp,
                "fixture:backlight".to_owned(),
                error.diagnostic(),
            )
        });

        let network_runner =
            FixtureCommandRunner::from_dir(root.join("commands")).unwrap_or_default();
        let mut network_provider =
            NetworkProvider::new(network_runner).with_timestamp(Some(timestamp));
        let network = network_provider
            .snapshot()
            .unwrap_or_else(|error| NetworkSnapshot::unavailable(timestamp, error.diagnostic()));

        let bluetooth_runner =
            FixtureCommandRunner::from_dir(root.join("commands")).unwrap_or_default();
        let mut bluetooth_provider =
            BluetoothProvider::new(bluetooth_runner).with_timestamp(Some(timestamp));
        let bluetooth = bluetooth_provider
            .snapshot()
            .unwrap_or_else(|error| BluetoothSnapshot::unavailable(timestamp, error.diagnostic()));

        let audio_runner =
            FixtureCommandRunner::from_dir(root.join("commands")).unwrap_or_default();
        let mut audio_provider = AudioProvider::new(audio_runner).with_timestamp(Some(timestamp));
        let audio = audio_provider
            .snapshot()
            .unwrap_or_else(|error| AudioSnapshot::unavailable(timestamp, error.diagnostic()));

        let mut session_provider =
            SessionProvider::new(FixtureCommandRunner::default()).with_timestamp(Some(timestamp));
        let session = session_provider
            .snapshot()
            .unwrap_or_else(|error| SessionSnapshot {
                meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, "systemd-session"),
                destructive_actions_enabled: false,
                issues: vec![error.diagnostic()],
            });

        Self {
            battery,
            backlight,
            network,
            bluetooth,
            audio,
            clock,
            session,
        }
    }

    pub fn to_json_pretty(&self) -> String {
        let mut out = String::new();
        out.push_str("{\n  \"schema\": \"nuraloumi.probe.v1\",\n");
        write_battery(&mut out, &self.battery);
        out.push_str(",\n");
        write_backlight(&mut out, &self.backlight);
        out.push_str(",\n");
        write_network(&mut out, &self.network);
        out.push_str(",\n");
        write_bluetooth(&mut out, &self.bluetooth);
        out.push_str(",\n");
        write_audio(&mut out, &self.audio);
        out.push_str(",\n");
        write_clock(&mut out, &self.clock);
        out.push_str(",\n");
        write_session(&mut out, &self.session);
        out.push_str("\n}\n");
        out
    }
}

pub fn action_result_json(result: &ActionResult) -> String {
    let mut out = String::from("{\"executed\":");
    out.push_str(if result.executed { "true" } else { "false" });
    out.push_str(",\"dry_run\":");
    out.push_str(if result.dry_run { "true" } else { "false" });
    out.push_str(",\"message\":");
    push_json_string(&mut out, &result.message);
    out.push_str("}\n");
    out
}

fn write_meta(out: &mut String, meta: &SnapshotMeta) {
    out.push_str("\"meta\":{\"timestamp_ms\":");
    let _ = write!(out, "{}", meta.timestamp_ms);
    out.push_str(",\"health\":");
    push_json_string(out, meta.health.as_str());
    out.push_str(",\"stale\":");
    out.push_str(if meta.stale { "true" } else { "false" });
    out.push_str(",\"source\":");
    push_json_string(out, &meta.source);
    out.push('}');
}

fn write_battery(out: &mut String, snapshot: &BatterySnapshot) {
    out.push_str("  \"battery\": {");
    write_meta(out, &snapshot.meta);
    out.push_str(",\"supplies\":[");
    for (index, supply) in snapshot.supplies.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, &supply.name);
        out.push_str(",\"kind\":");
        push_json_string(out, supply.kind.as_str());
        out.push_str(",\"capacity_percent\":");
        push_option_u8(out, supply.capacity_percent);
        out.push_str(",\"status\":");
        push_option_string(out, supply.status.as_deref());
        out.push_str(",\"charging\":");
        push_option_bool(out, supply.charging);
        out.push_str(",\"online\":");
        push_option_bool(out, supply.online);
        out.push('}');
    }
    out.push_str("],\"issues\":");
    write_strings(out, &snapshot.issues);
    out.push('}');
}

fn write_backlight(out: &mut String, snapshot: &BacklightSnapshot) {
    out.push_str("  \"backlight\": {");
    write_meta(out, &snapshot.meta);
    out.push_str(",\"devices\":[");
    for (index, device) in snapshot.devices.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"name\":");
        push_json_string(out, &device.name);
        let _ = write!(
            out,
            ",\"brightness\":{},\"max_brightness\":{},\"percent\":{},\"writable\":{}",
            device.brightness,
            device.max_brightness,
            device.percent,
            if device.writable { "true" } else { "false" }
        );
        out.push('}');
    }
    out.push_str("],\"issues\":");
    write_strings(out, &snapshot.issues);
    out.push('}');
}

fn write_network(out: &mut String, snapshot: &NetworkSnapshot) {
    out.push_str("  \"network\": {");
    write_meta(out, &snapshot.meta);
    out.push_str(",\"connected\":");
    out.push_str(if snapshot.connected { "true" } else { "false" });
    out.push_str(",\"interface\":");
    push_option_string(out, snapshot.interface.as_deref());
    out.push_str(",\"ssid\":");
    push_option_string(out, snapshot.ssid.as_deref());
    out.push_str(",\"signal_percent\":");
    push_option_u8(out, snapshot.signal_percent);
    out.push_str(",\"issues\":");
    write_strings(out, &snapshot.issues);
    out.push('}');
}

fn write_bluetooth(out: &mut String, snapshot: &BluetoothSnapshot) {
    out.push_str("  \"bluetooth\": {");
    write_meta(out, &snapshot.meta);
    out.push_str(",\"powered\":");
    push_option_bool(out, snapshot.powered);
    out.push_str(",\"devices\":[");
    for (index, device) in snapshot.devices.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        out.push_str("{\"address\":");
        push_json_string(out, &device.address);
        out.push_str(",\"label\":");
        push_json_string(out, &device.label);
        out.push_str(",\"paired\":");
        out.push_str(if device.paired { "true" } else { "false" });
        out.push_str(",\"connected\":");
        out.push_str(if device.connected { "true" } else { "false" });
        out.push('}');
    }
    out.push_str("],\"issues\":");
    write_strings(out, &snapshot.issues);
    out.push('}');
}

fn write_audio(out: &mut String, snapshot: &AudioSnapshot) {
    out.push_str("  \"audio\": {");
    write_meta(out, &snapshot.meta);
    out.push_str(",\"backend\":");
    push_json_string(out, &snapshot.backend);
    out.push_str(",\"volume_percent\":");
    match snapshot.volume_percent {
        Some(value) => {
            let _ = write!(out, "{value}");
        }
        None => out.push_str("null"),
    }
    out.push_str(",\"muted\":");
    push_option_bool(out, snapshot.muted);
    out.push_str(",\"issues\":");
    write_strings(out, &snapshot.issues);
    out.push('}');
}

fn write_clock(out: &mut String, snapshot: &ClockSnapshot) {
    out.push_str("  \"clock\": {");
    write_meta(out, &snapshot.meta);
    let _ = write!(
        out,
        ",\"unix_timestamp_ms\":{},\"time_label\":",
        snapshot.unix_timestamp_ms
    );
    push_json_string(out, &snapshot.time_label);
    out.push_str(",\"issues\":");
    write_strings(out, &snapshot.issues);
    out.push('}');
}

fn write_session(out: &mut String, snapshot: &SessionSnapshot) {
    out.push_str("  \"session\": {");
    write_meta(out, &snapshot.meta);
    out.push_str(",\"destructive_actions_enabled\":");
    out.push_str(if snapshot.destructive_actions_enabled {
        "true"
    } else {
        "false"
    });
    out.push_str(",\"issues\":");
    write_strings(out, &snapshot.issues);
    out.push('}');
}

fn write_strings(out: &mut String, values: &[String]) {
    out.push('[');
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        push_json_string(out, value);
    }
    out.push(']');
}

fn push_option_string(out: &mut String, value: Option<&str>) {
    match value {
        Some(value) => push_json_string(out, value),
        None => out.push_str("null"),
    }
}

fn push_option_u8(out: &mut String, value: Option<u8>) {
    match value {
        Some(value) => {
            let _ = write!(out, "{value}");
        }
        None => out.push_str("null"),
    }
}

fn push_option_bool(out: &mut String, value: Option<bool>) {
    match value {
        Some(true) => out.push_str("true"),
        Some(false) => out.push_str("false"),
        None => out.push_str("null"),
    }
}

fn push_json_string(out: &mut String, value: &str) {
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            ch if ch.is_control() => {
                let _ = write!(out, "\\u{:04x}", ch as u32);
            }
            ch => out.push(ch),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_result_json_escapes_messages() {
        let value = action_result_json(&ActionResult::dry_run("a\"b\nc"));
        assert_eq!(
            value,
            "{\"executed\":false,\"dry_run\":true,\"message\":\"a\\\"b\\nc\"}\n"
        );
    }
}
