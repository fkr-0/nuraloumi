//! NuraLoumi shell/runtime integration.
//!
//! The renderer-neutral semantic model, actions, navigation, search, validation,
//! theme and motion contracts live in nuraloumi-core. This crate owns only shell
//! runtime concerns such as focus, confirmation arming, surface lifetime, and
//! provider dispatch. No system command is constructed or executed by core or
//! renderer code.

pub use nuraloumi_core::{
    Confirmation, MenuAction, MenuItem, MenuItemKind, MenuModel, MenuState, NavigationOutcome,
    SemanticInput,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanelEdge {
    Top,
    Bottom,
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Theme {
    Dark,
    Light,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellConfig {
    pub panel_edge: PanelEdge,
    pub panel_height: u32,
    pub menu_width: u32,
    pub row_height: u32,
    pub theme: Theme,
    pub reduced_motion: bool,
}

impl Default for ShellConfig {
    fn default() -> Self {
        Self {
            panel_edge: PanelEdge::Top,
            panel_height: 48,
            menu_width: 480,
            row_height: 52,
            theme: Theme::Dark,
            reduced_motion: false,
        }
    }
}

impl ShellConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !(40..=72).contains(&self.panel_height) {
            return Err(format!(
                "panel_height {} is invalid; expected 40..=72 logical px",
                self.panel_height
            ));
        }
        if !(320..=720).contains(&self.menu_width) {
            return Err(format!(
                "menu_width {} is invalid; expected 320..=720 logical px",
                self.menu_width
            ));
        }
        if !(40..=72).contains(&self.row_height) {
            return Err(format!(
                "row_height {} is invalid; expected 40..=72 logical px",
                self.row_height
            ));
        }
        Ok(())
    }
}

pub fn load_config(path: impl AsRef<Path>) -> Result<ShellConfig, String> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read config {}: {err}", path.display()))?;
    let config: ShellConfig = parse_by_extension(path, &text)?;
    config.validate()?;
    Ok(config)
}

pub fn load_menu(path: impl AsRef<Path>) -> Result<MenuModel, String> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read menu {}: {err}", path.display()))?;
    let menu: MenuModel = parse_by_extension(path, &text)?;
    menu.validate().map_err(|error| error.to_string())?;
    Ok(menu)
}

fn parse_by_extension<T>(path: &Path, text: &str) -> Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("json") => serde_json::from_str(text)
            .map_err(|err| format!("invalid JSON in {}: {err}", path.display())),
        Some("toml") => {
            toml::from_str(text).map_err(|err| format!("invalid TOML in {}: {err}", path.display()))
        }
        other => Err(format!(
            "unsupported file extension {:?} for {}; expected .json or .toml",
            other,
            path.display()
        )),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValueState {
    Ready,
    Unavailable,
    Stale,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderValue {
    pub state: ValueState,
    #[serde(default)]
    pub value: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
}

impl ProviderValue {
    fn subtitle(&self) -> String {
        match self.state {
            ValueState::Ready => self.value.clone().unwrap_or_else(|| "Ready".into()),
            ValueState::Unavailable => format!(
                "Unavailable{}",
                self.message
                    .as_deref()
                    .map(|message| format!(" — {message}"))
                    .unwrap_or_default()
            ),
            ValueState::Stale => format!(
                "Stale{}",
                self.value
                    .as_deref()
                    .map(|value| format!(" — {value}"))
                    .unwrap_or_default()
            ),
            ValueState::Error => format!(
                "Error{}",
                self.message
                    .as_deref()
                    .map(|message| format!(" — {message}"))
                    .unwrap_or_default()
            ),
        }
    }

    fn available(&self) -> bool {
        self.state == ValueState::Ready
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WifiNetworkEntry {
    pub ssid: String,
    #[serde(default)]
    pub signal_percent: Option<u8>,
    #[serde(default)]
    pub secured: bool,
    #[serde(default)]
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BluetoothDeviceEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub paired: bool,
    #[serde(default)]
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskEntry {
    pub id: String,
    pub label: String,
    pub state: String,
    #[serde(default)]
    pub cpu_percent: Option<u8>,
    #[serde(default)]
    pub memory_mib: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowEntry {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub app_id: Option<String>,
    #[serde(default)]
    pub focused: bool,
    #[serde(default)]
    pub fullscreen: bool,
}

fn default_bluetooth_value() -> ProviderValue {
    ProviderValue {
        state: ValueState::Unavailable,
        value: None,
        message: Some("Bluetooth provider not connected".into()),
    }
}

const fn default_brightness_writable() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FixtureSnapshot {
    pub network: ProviderValue,
    pub audio: ProviderValue,
    pub battery: ProviderValue,
    pub clock: ProviderValue,
    pub brightness: ProviderValue,
    #[serde(default = "default_brightness_writable")]
    pub brightness_writable: bool,
    #[serde(default = "default_bluetooth_value")]
    pub bluetooth: ProviderValue,
    #[serde(default)]
    pub wifi_networks: Vec<WifiNetworkEntry>,
    #[serde(default)]
    pub bluetooth_devices: Vec<BluetoothDeviceEntry>,
    #[serde(default)]
    pub tasks: Vec<TaskEntry>,
    #[serde(default)]
    pub windows: Vec<WindowEntry>,
}

impl Default for FixtureSnapshot {
    fn default() -> Self {
        Self {
            network: ProviderValue {
                state: ValueState::Ready,
                value: Some("Lab Wi-Fi · 82%".into()),
                message: None,
            },
            audio: ProviderValue {
                state: ValueState::Ready,
                value: Some("46% · speaker".into()),
                message: None,
            },
            battery: ProviderValue {
                state: ValueState::Ready,
                value: Some("73%".into()),
                message: None,
            },
            clock: ProviderValue {
                state: ValueState::Ready,
                value: Some("23:42".into()),
                message: None,
            },
            brightness: ProviderValue {
                state: ValueState::Ready,
                value: Some("62%".into()),
                message: None,
            },
            brightness_writable: true,
            bluetooth: ProviderValue {
                state: ValueState::Ready,
                value: Some("On · 1 connected".into()),
                message: None,
            },
            wifi_networks: vec![
                WifiNetworkEntry {
                    ssid: "Lab Wi-Fi".into(),
                    signal_percent: Some(82),
                    secured: true,
                    connected: true,
                },
                WifiNetworkEntry {
                    ssid: "Workshop".into(),
                    signal_percent: Some(61),
                    secured: true,
                    connected: false,
                },
            ],
            bluetooth_devices: vec![
                BluetoothDeviceEntry {
                    id: "headphones".into(),
                    label: "Headphones".into(),
                    paired: true,
                    connected: true,
                },
                BluetoothDeviceEntry {
                    id: "phone".into(),
                    label: "Phone".into(),
                    paired: true,
                    connected: false,
                },
            ],
            tasks: vec![
                TaskEntry {
                    id: "labwc".into(),
                    label: "labwc".into(),
                    state: "Running".into(),
                    cpu_percent: Some(2),
                    memory_mib: Some(18),
                },
                TaskEntry {
                    id: "nuraloumi-panel".into(),
                    label: "NuraLoumi panel".into(),
                    state: "Running".into(),
                    cpu_percent: Some(1),
                    memory_mib: Some(24),
                },
                TaskEntry {
                    id: "foot".into(),
                    label: "Terminal".into(),
                    state: "Sleeping".into(),
                    cpu_percent: Some(0),
                    memory_mib: Some(15),
                },
            ],
            windows: vec![
                WindowEntry {
                    id: "terminal".into(),
                    title: "Terminal — foot".into(),
                    app_id: Some("foot".into()),
                    focused: true,
                    fullscreen: false,
                },
                WindowEntry {
                    id: "files".into(),
                    title: "Files".into(),
                    app_id: Some("thunar".into()),
                    focused: false,
                    fullscreen: false,
                },
            ],
        }
    }
}

pub fn load_fixture_snapshot(path: impl AsRef<Path>) -> Result<FixtureSnapshot, String> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .map_err(|err| format!("failed to read fixture {}: {err}", path.display()))?;
    parse_by_extension(path, &text)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuFamily {
    Launcher,
    Network,
    Bluetooth,
    Display,
    Audio,
    Power,
    Tasks,
    Windows,
    System,
}

pub fn build_family(family: MenuFamily, snapshot: &FixtureSnapshot) -> MenuModel {
    match family {
        MenuFamily::Launcher => build_launcher_menu(),
        MenuFamily::Network => build_network_menu(snapshot),
        MenuFamily::Bluetooth => build_bluetooth_menu(snapshot),
        MenuFamily::Display => build_display_menu(snapshot),
        MenuFamily::Audio => build_audio_menu(snapshot),
        MenuFamily::Power => build_power_menu(snapshot),
        MenuFamily::Tasks => build_tasks_menu(snapshot),
        MenuFamily::Windows => build_windows_menu(snapshot),
        MenuFamily::System => build_system_menu(snapshot),
    }
}

pub fn build_launcher_menu() -> MenuModel {
    MenuModel {
        id: "launcher".into(),
        title: "Apps".into(),
        items: vec![
            status(
                "launcher.search",
                "Search applications…",
                "Type while search is focused",
            ),
            section("launcher.apps", "Applications"),
            action("app.terminal", "Terminal", "app.launch.terminal"),
            action("app.files", "Files", "app.launch.files"),
            action("app.browser", "Browser", "app.launch.browser"),
            action("app.music", "Music", "app.launch.music"),
            section("launcher.recent", "Recent"),
            status(
                "recent.status",
                "Recent items",
                "Unavailable — history provider not connected",
            ),
        ],
    }
}

pub fn build_network_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    let available = snapshot.network.available();
    let mut items = vec![
        section("network.summary", "Wi-Fi"),
        provider_status("network.state", "Connection", &snapshot.network),
        toggle_action(
            "network.toggle",
            "Wi-Fi radio",
            "network.wifi",
            available,
            None,
            available.then_some("Toggle radio through the network provider"),
        ),
        action_with_subtitle(
            "network.rescan",
            "Scan again",
            "network.rescan",
            available,
            (!available).then_some("Provider unavailable or stale"),
        ),
        section("network.scan", "Networks"),
    ];

    if snapshot.wifi_networks.is_empty() {
        items.push(status(
            "network.empty",
            "No scan results",
            "Fixture/provider did not report visible networks",
        ));
    } else {
        items.extend(
            snapshot
                .wifi_networks
                .iter()
                .take(24)
                .enumerate()
                .map(|(index, network)| {
                    let signal = network
                        .signal_percent
                        .map(|value| format!("{value}%"))
                        .unwrap_or_else(|| "signal unknown".into());
                    let security = if network.secured { "secured" } else { "open" };
                    let item_id = format!("network.ssid.{index}");
                    if network.connected {
                        status(
                            &item_id,
                            &network.ssid,
                            &format!("Connected · {signal} · {security}"),
                        )
                    } else {
                        custom_action(
                            &item_id,
                            &network.ssid,
                            Some(&format!("{signal} · {security}")),
                            "network.connect",
                            &network.ssid,
                            available,
                        )
                    }
                }),
        );
    }

    MenuModel {
        id: "network".into(),
        title: "Wi-Fi".into(),
        items,
    }
}

pub fn build_bluetooth_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    let available = snapshot.bluetooth.available();
    let mut items = vec![
        section("bluetooth.summary", "Bluetooth"),
        provider_status("bluetooth.state", "Adapter", &snapshot.bluetooth),
        toggle_action(
            "bluetooth.toggle",
            "Bluetooth radio",
            "bluetooth.radio",
            available,
            snapshot
                .bluetooth
                .value
                .as_deref()
                .map(|value| value.trim_start().starts_with("On")),
            available.then_some("Toggle adapter power"),
        ),
        section("bluetooth.devices", "Devices"),
    ];

    if snapshot.bluetooth_devices.is_empty() {
        items.push(status(
            "bluetooth.empty",
            "No devices",
            "Live discovery is not connected; fixture list is empty",
        ));
    } else {
        items.extend(snapshot.bluetooth_devices.iter().take(24).map(|device| {
            let subtitle = match (device.connected, device.paired) {
                (true, _) => "Connected",
                (false, true) => "Paired · tap to connect",
                (false, false) => "Known · tap to pair/connect",
            };
            let kind = if device.connected {
                "bluetooth.disconnect"
            } else {
                "bluetooth.connect"
            };
            custom_action(
                &format!("bluetooth.device.{}", device.id),
                &device.label,
                Some(subtitle),
                kind,
                &device.id,
                available,
            )
        }));
    }

    MenuModel {
        id: "bluetooth".into(),
        title: "Bluetooth".into(),
        items,
    }
}

pub fn build_display_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    let brightness_available = snapshot.brightness.available() && snapshot.brightness_writable;
    let focused_window = snapshot.windows.iter().find(|window| window.focused);
    let mut items = vec![
        section("display.brightness", "Brightness"),
        provider_status(
            "display.brightness.state",
            "Brightness",
            &snapshot.brightness,
        ),
        adjustable(
            "display.brightness.down",
            "Brightness −10%",
            "system.brightness",
            -10,
            brightness_available,
        ),
        adjustable(
            "display.brightness.up",
            "Brightness +10%",
            "system.brightness",
            10,
            brightness_available,
        ),
        section("display.window", "Active window"),
    ];

    if let Some(window) = focused_window {
        items.push(toggle_action(
            "display.fullscreen",
            "Fullscreen",
            &format!("window.fullscreen:{}", window.id),
            true,
            Some(window.fullscreen),
            Some(if window.fullscreen {
                "Currently fullscreen"
            } else {
                "Currently windowed"
            }),
        ));
        items.push(status(
            "display.active",
            &window.title,
            window.app_id.as_deref().unwrap_or("active window"),
        ));
    } else {
        items.push(status(
            "display.fullscreen.unavailable",
            "Fullscreen",
            "No focused window in the current snapshot",
        ));
    }

    MenuModel {
        id: "display".into(),
        title: "Display".into(),
        items,
    }
}

pub fn build_audio_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    let available = snapshot.audio.available();
    MenuModel {
        id: "audio".into(),
        title: "Speaker volume".into(),
        items: vec![
            section("audio.summary", "Speaker"),
            provider_status("audio.state", "Volume", &snapshot.audio),
            toggle_action(
                "audio.mute",
                "Mute",
                "audio.mute",
                available,
                snapshot
                    .audio
                    .value
                    .as_deref()
                    .map(|value| value.to_ascii_lowercase().contains("muted")),
                available.then_some("Toggle default speaker mute"),
            ),
            adjustable("audio.down", "Volume −5%", "audio.volume", -5, available),
            adjustable("audio.up", "Volume +5%", "audio.volume", 5, available),
        ],
    }
}

pub fn build_power_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    MenuModel {
        id: "power".into(),
        title: "Power".into(),
        items: vec![
            section("power.summary", "Power"),
            provider_status("power.battery", "Battery", &snapshot.battery),
            status(
                "power.safety",
                "Safety",
                "Power actions require confirmation; provider defaults remain dry-run",
            ),
            confirm_action("power.suspend", "Suspend", "system.suspend"),
            confirm_action("power.restart", "Restart…", "system.restart"),
            confirm_action("power.poweroff", "Power off…", "system.poweroff"),
        ],
    }
}

pub fn build_tasks_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    let mut items = vec![
        section("tasks.summary", "Tasks"),
        status(
            "tasks.concept",
            "Conceptual task viewer",
            "Fixture snapshot only · inspect actions do not kill processes",
        ),
    ];

    if snapshot.tasks.is_empty() {
        items.push(status(
            "tasks.empty",
            "No tasks",
            "Process/task provider is not connected",
        ));
    } else {
        items.extend(snapshot.tasks.iter().take(32).map(|task| {
            let mut detail = vec![task.state.clone()];
            if let Some(cpu) = task.cpu_percent {
                detail.push(format!("CPU {cpu}%"));
            }
            if let Some(memory) = task.memory_mib {
                detail.push(format!("{memory} MiB"));
            }
            custom_action(
                &format!("tasks.item.{}", task.id),
                &task.label,
                Some(&detail.join(" · ")),
                "task.inspect",
                &task.id,
                true,
            )
        }));
    }

    MenuModel {
        id: "tasks".into(),
        title: "Tasks".into(),
        items,
    }
}

pub fn build_windows_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    let mut items = vec![
        section("windows.summary", "Windows"),
        status(
            "windows.concept",
            "Conceptual window list",
            "Fixture/compositor snapshot · focus/fullscreen/close are semantic actions only",
        ),
    ];

    if let Some(window) = snapshot.windows.iter().find(|window| window.focused) {
        items.push(toggle_action(
            "windows.fullscreen",
            "Toggle fullscreen",
            &format!("window.fullscreen:{}", window.id),
            true,
            Some(window.fullscreen),
            Some(&window.title),
        ));
        items.push(confirm_custom_action(
            "windows.close",
            "Close active window…",
            Some(&window.title),
            "window.close",
            &window.id,
            true,
        ));
    }

    items.push(section("windows.list", "Open windows"));
    if snapshot.windows.is_empty() {
        items.push(status(
            "windows.empty",
            "No windows",
            "Foreign-toplevel/compositor source is not connected",
        ));
    } else {
        items.extend(snapshot.windows.iter().take(32).map(|window| {
            let mut detail = Vec::new();
            if window.focused {
                detail.push("Focused");
            }
            if window.fullscreen {
                detail.push("Fullscreen");
            }
            if let Some(app_id) = window.app_id.as_deref() {
                detail.push(app_id);
            }
            let subtitle = if detail.is_empty() {
                "Open".to_owned()
            } else {
                detail.join(" · ")
            };
            custom_action(
                &format!("windows.item.{}", window.id),
                &window.title,
                Some(&subtitle),
                "window.focus",
                &window.id,
                true,
            )
        }));
    }

    MenuModel {
        id: "windows".into(),
        title: "Windows".into(),
        items,
    }
}

pub fn build_system_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    MenuModel {
        id: "system".into(),
        title: "System".into(),
        items: vec![
            section("system.summary", "Quick status"),
            provider_status("system.network", "Wi-Fi", &snapshot.network),
            provider_status("system.bluetooth", "Bluetooth", &snapshot.bluetooth),
            provider_status(
                "system.brightness.state",
                "Brightness",
                &snapshot.brightness,
            ),
            provider_status("system.audio", "Speaker volume", &snapshot.audio),
            section("system.menus", "Controls"),
            custom_action(
                "system.open.network",
                "Wi-Fi",
                Some("Networks and radio"),
                "menu.open",
                "wifi",
                true,
            ),
            custom_action(
                "system.open.bluetooth",
                "Bluetooth",
                Some("Adapter and devices"),
                "menu.open",
                "bluetooth",
                true,
            ),
            custom_action(
                "system.open.display",
                "Display",
                Some("Brightness and fullscreen"),
                "menu.open",
                "display",
                true,
            ),
            custom_action(
                "system.open.audio",
                "Speaker volume",
                Some("Volume and mute"),
                "menu.open",
                "audio",
                true,
            ),
            custom_action(
                "system.open.tasks",
                "Tasks",
                Some("Conceptual task viewer"),
                "menu.open",
                "tasks",
                true,
            ),
            custom_action(
                "system.open.windows",
                "Windows",
                Some("Conceptual window list"),
                "menu.open",
                "windows",
                true,
            ),
            section("system.session", "Power"),
            confirm_action("system.suspend", "Suspend", "system.suspend"),
            confirm_action("system.restart", "Restart…", "system.restart"),
            confirm_action("system.poweroff", "Power off…", "system.poweroff"),
        ],
    }
}

fn section(id: &str, label: &str) -> MenuItem {
    MenuItem::section(id, label)
}

fn status(id: &str, label: &str, subtitle: &str) -> MenuItem {
    MenuItem::status(id, label).with_subtitle(subtitle)
}

fn provider_status(id: &str, label: &str, value: &ProviderValue) -> MenuItem {
    status(id, label, &value.subtitle())
}

fn action(id: &str, label: &str, action_id: &str) -> MenuItem {
    MenuItem::action(
        id,
        label,
        MenuAction::Activate {
            id: action_id.into(),
        },
    )
}

fn action_with_subtitle(
    id: &str,
    label: &str,
    action_id: &str,
    enabled: bool,
    subtitle: Option<&str>,
) -> MenuItem {
    let mut item = MenuItem::action(
        id,
        label,
        MenuAction::Activate {
            id: action_id.into(),
        },
    );
    item.subtitle = subtitle.map(str::to_owned);
    if !enabled {
        item.enabled = false;
        item.action = None;
    }
    item
}

fn toggle_action(
    id: &str,
    label: &str,
    action_id: &str,
    enabled: bool,
    checked: Option<bool>,
    subtitle: Option<&str>,
) -> MenuItem {
    MenuItem {
        id: id.into(),
        label: label.into(),
        subtitle: subtitle
            .map(str::to_owned)
            .or_else(|| (!enabled).then(|| "Provider unavailable or stale".into())),
        kind: MenuItemKind::Checkable,
        enabled,
        visible: true,
        action: enabled.then(|| MenuAction::Toggle {
            id: action_id.into(),
        }),
        children: Vec::new(),
        checked,
    }
}

fn custom_action(
    id: &str,
    label: &str,
    subtitle: Option<&str>,
    kind: &str,
    payload: &str,
    enabled: bool,
) -> MenuItem {
    let mut item = MenuItem::action(
        id,
        label,
        MenuAction::Custom {
            kind: kind.into(),
            payload: payload.into(),
        },
    );
    item.subtitle = subtitle
        .map(str::to_owned)
        .or_else(|| (!enabled).then(|| "Provider unavailable or stale".into()));
    if !enabled {
        item.enabled = false;
        item.action = None;
    }
    item
}

fn confirm_custom_action(
    id: &str,
    label: &str,
    subtitle: Option<&str>,
    kind: &str,
    payload: &str,
    enabled: bool,
) -> MenuItem {
    let body = subtitle.map(str::to_owned);
    let mut item = MenuItem::action(
        id,
        label,
        MenuAction::destructive(
            label,
            body,
            MenuAction::Custom {
                kind: kind.into(),
                payload: payload.into(),
            },
        ),
    );
    item.subtitle = subtitle
        .map(|value| format!("{value} · Requires confirmation"))
        .or_else(|| Some("Requires confirmation".into()));
    if !enabled {
        item.enabled = false;
        item.action = None;
    }
    item
}

fn adjustable(id: &str, label: &str, action_id: &str, delta: i32, enabled: bool) -> MenuItem {
    let mut item = MenuItem::action(
        id,
        label,
        MenuAction::Adjust {
            id: action_id.into(),
            delta,
        },
    );
    if !enabled {
        item.enabled = false;
        item.action = None;
        item.subtitle = Some("Provider unavailable or stale".into());
    }
    item
}

fn confirm_action(id: &str, label: &str, action_id: &str) -> MenuItem {
    MenuItem::action(
        id,
        label,
        MenuAction::destructive(
            label,
            Some("Requires confirmation".into()),
            MenuAction::Activate {
                id: action_id.into(),
            },
        ),
    )
    .with_subtitle("Requires confirmation")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellInput {
    Semantic(SemanticInput),
    SearchFocus(bool),
}

impl From<SemanticInput> for ShellInput {
    fn from(input: SemanticInput) -> Self {
        Self::Semantic(input)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ActionReport {
    Dispatched {
        item_id: String,
        action: MenuAction,
        confirmed: bool,
    },
    ConfirmationRequired {
        item_id: String,
        confirmation: Confirmation,
        action: MenuAction,
    },
    ConfirmationCancelled {
        item_id: String,
    },
    NavigationChanged {
        path: Vec<String>,
    },
    SearchChanged {
        query: String,
        focused: bool,
    },
    SurfaceClosed,
    Ignored {
        reason: String,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HitRegion {
    pub region_id: String,
    pub item_id: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformKey {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Backspace,
    Character(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlatformEvent {
    PointerButton {
        region: Option<HitRegion>,
        pressed: bool,
    },
    TouchDown {
        id: i32,
        region: Option<HitRegion>,
    },
    TouchUp {
        id: i32,
        region: Option<HitRegion>,
    },
    Key {
        key: PlatformKey,
        pressed: bool,
    },
    Close,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellState {
    pub menu: MenuModel,
    /// Canonical semantic navigation/search state from nuraloumi-core.
    pub state: MenuState,
    #[serde(default)]
    pub search_focused: bool,
    #[serde(default)]
    pub reduced_motion: bool,
    #[serde(default)]
    pub closed: bool,
    #[serde(default)]
    pub pending_confirmation: Option<String>,
    #[serde(skip)]
    pointer_pressed_region: Option<HitRegion>,
    #[serde(skip)]
    touch_pressed_regions: BTreeMap<i32, HitRegion>,
}

impl ShellState {
    pub fn new(menu: MenuModel, reduced_motion: bool) -> Result<Self, String> {
        menu.validate().map_err(|error| error.to_string())?;
        let state = MenuState::new(&menu);
        Ok(Self {
            menu,
            state,
            search_focused: false,
            reduced_motion,
            closed: false,
            pending_confirmation: None,
            pointer_pressed_region: None,
            touch_pressed_regions: BTreeMap::new(),
        })
    }

    pub fn refresh_menu(&mut self, menu: MenuModel) -> Result<(), String> {
        menu.validate().map_err(|error| error.to_string())?;
        self.menu = menu;
        self.state.normalize(&self.menu);
        // Never carry confirmation authorization across a provider/model refresh.
        self.pending_confirmation = None;
        Ok(())
    }

    pub fn focus_search(&mut self, focused: bool) -> ActionReport {
        self.search_focused = focused;
        ActionReport::SearchChanged {
            query: self.state.query.clone(),
            focused,
        }
    }

    pub fn apply_input(&mut self, input: ShellInput) -> ActionReport {
        match input {
            ShellInput::Semantic(input) => self.apply_semantic(input),
            ShellInput::SearchFocus(focused) => self.focus_search(focused),
        }
    }

    pub fn apply_semantic(&mut self, input: SemanticInput) -> ActionReport {
        if matches!(&input, SemanticInput::Text(_) | SemanticInput::Backspace)
            && !self.search_focused
        {
            return ActionReport::Ignored {
                reason: "search input is not focused".into(),
            };
        }

        if matches!(&input, SemanticInput::Back | SemanticInput::Escape) {
            if self.search_focused {
                return self.focus_search(false);
            }
            if let Some(item_id) = self.pending_confirmation.take() {
                return ActionReport::ConfirmationCancelled { item_id };
            }
        }

        let previous_selection = self.state.selected_id.clone();
        let outcome = self.state.handle(&self.menu, input);
        if self.state.selected_id != previous_selection {
            self.pending_confirmation = None;
        }
        self.report_navigation_outcome(outcome)
    }

    fn report_navigation_outcome(&mut self, outcome: NavigationOutcome) -> ActionReport {
        match outcome {
            NavigationOutcome::Noop => ActionReport::Ignored {
                reason: "semantic input produced no action".into(),
            },
            NavigationOutcome::SelectionChanged { .. }
            | NavigationOutcome::SubmenuEntered { .. }
            | NavigationOutcome::SubmenuExited { .. } => ActionReport::NavigationChanged {
                path: self.state.path.clone(),
            },
            NavigationOutcome::QueryChanged { query, .. } => ActionReport::SearchChanged {
                query,
                focused: self.search_focused,
            },
            NavigationOutcome::ActionRequested { item_id, action } => {
                if action == MenuAction::Close {
                    self.closed = true;
                    ActionReport::SurfaceClosed
                } else {
                    ActionReport::Dispatched {
                        item_id,
                        action,
                        confirmed: false,
                    }
                }
            }
            NavigationOutcome::ConfirmationRequested {
                item_id,
                confirmation,
                action,
            } => {
                if self.pending_confirmation.as_deref() == Some(item_id.as_str()) {
                    self.pending_confirmation = None;
                    ActionReport::Dispatched {
                        item_id,
                        action,
                        confirmed: true,
                    }
                } else {
                    self.pending_confirmation = Some(item_id.clone());
                    ActionReport::ConfirmationRequired {
                        item_id,
                        confirmation,
                        action,
                    }
                }
            }
            NavigationOutcome::CloseRequested => {
                self.closed = true;
                ActionReport::SurfaceClosed
            }
        }
    }

    pub fn handle_platform_event(&mut self, event: PlatformEvent) -> Option<ActionReport> {
        match event {
            PlatformEvent::PointerButton { region, pressed } => {
                if pressed {
                    self.pointer_pressed_region = region;
                    None
                } else {
                    let pressed_region = self.pointer_pressed_region.take();
                    match (pressed_region, region) {
                        (Some(pressed), Some(released))
                            if pressed.region_id == released.region_id
                                && pressed.item_id == released.item_id =>
                        {
                            Some(self.activate_item(&released.item_id))
                        }
                        _ => Some(ActionReport::Ignored {
                            reason: "pointer release did not match pressed hit region".into(),
                        }),
                    }
                }
            }
            PlatformEvent::TouchDown { id, region } => {
                if let Some(region) = region {
                    self.touch_pressed_regions.insert(id, region);
                }
                None
            }
            PlatformEvent::TouchUp { id, region } => {
                let pressed = self.touch_pressed_regions.remove(&id);
                match (pressed, region) {
                    (Some(pressed), Some(released))
                        if pressed.region_id == released.region_id
                            && pressed.item_id == released.item_id =>
                    {
                        Some(self.activate_item(&released.item_id))
                    }
                    _ => Some(ActionReport::Ignored {
                        reason: "touch release did not match pressed hit region".into(),
                    }),
                }
            }
            PlatformEvent::Key { key, pressed } => {
                if !pressed {
                    return None;
                }
                Some(self.apply_semantic(match key {
                    PlatformKey::Up => SemanticInput::Up,
                    PlatformKey::Down => SemanticInput::Down,
                    PlatformKey::Left => SemanticInput::Left,
                    PlatformKey::Right => SemanticInput::Right,
                    PlatformKey::Enter => SemanticInput::Activate,
                    PlatformKey::Escape => SemanticInput::Back,
                    PlatformKey::Backspace => SemanticInput::Backspace,
                    PlatformKey::Character(text) => SemanticInput::Text(text),
                }))
            }
            PlatformEvent::Close => {
                self.closed = true;
                Some(ActionReport::SurfaceClosed)
            }
        }
    }

    pub fn hit_regions(&self) -> Vec<HitRegion> {
        self.state
            .visible_items(&self.menu)
            .into_iter()
            .filter(|item| item.is_actionable())
            .map(|item| HitRegion {
                region_id: format!("row:{}", item.id),
                item_id: item.id.clone(),
            })
            .collect()
    }

    fn activate_item(&mut self, item_id: &str) -> ActionReport {
        let actionable = self
            .state
            .visible_items(&self.menu)
            .into_iter()
            .any(|item| item.id == item_id && item.is_actionable());
        if !actionable {
            return ActionReport::Ignored {
                reason: format!("hit target {item_id} is not actionable"),
            };
        }
        if self.state.selected_id.as_deref() != Some(item_id) {
            self.pending_confirmation = None;
        }
        self.state.selected_id = Some(item_id.to_owned());
        self.apply_semantic(SemanticInput::Activate)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelAffordance {
    pub id: String,
    pub label: String,
    pub value: Option<String>,
    pub state: ValueState,
}

pub fn panel_affordances(snapshot: &FixtureSnapshot) -> Vec<PanelAffordance> {
    vec![
        PanelAffordance {
            id: "apps".into(),
            label: "Apps".into(),
            value: None,
            state: ValueState::Ready,
        },
        PanelAffordance {
            id: "network".into(),
            label: "Network".into(),
            value: snapshot.network.value.clone(),
            state: snapshot.network.state,
        },
        PanelAffordance {
            id: "audio".into(),
            label: "Audio".into(),
            value: snapshot.audio.value.clone(),
            state: snapshot.audio.state,
        },
        PanelAffordance {
            id: "battery".into(),
            label: "Battery".into(),
            value: snapshot.battery.value.clone(),
            state: snapshot.battery.state,
        },
        PanelAffordance {
            id: "clock".into(),
            label: "Clock".into(),
            value: snapshot.clock.value.clone(),
            state: snapshot.clock.state,
        },
    ]
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanelController {
    pub active_menu: Option<MenuFamily>,
    pub keyboard_focus: bool,
}

impl PanelController {
    pub fn open_menu(&mut self, family: MenuFamily, intentionally_interactive: bool) {
        self.active_menu = Some(family);
        self.keyboard_focus = intentionally_interactive;
    }

    pub fn close_menu(&mut self) {
        self.active_menu = None;
        self.keyboard_focus = false;
    }
}

pub fn parse_family(value: &str) -> Result<MenuFamily, String> {
    match value {
        "launcher" | "apps" => Ok(MenuFamily::Launcher),
        "network" | "net" | "wifi" | "wi-fi" => Ok(MenuFamily::Network),
        "bluetooth" | "bt" => Ok(MenuFamily::Bluetooth),
        "display" | "brightness" | "fullscreen" | "full-screen" => Ok(MenuFamily::Display),
        "audio" | "speaker" | "sound" | "volume" => Ok(MenuFamily::Audio),
        "power" | "session" => Ok(MenuFamily::Power),
        "tasks" | "task" | "task-viewer" => Ok(MenuFamily::Tasks),
        "windows" | "window" | "window-list" => Ok(MenuFamily::Windows),
        "system" | "battery" | "clock" | "controls" => Ok(MenuFamily::System),
        other => Err(format!(
            "unknown menu family {other:?}; expected launcher, wifi, bluetooth, display, audio, power, tasks, windows, or system"
        )),
    }
}

/// Human-readable live integration status for diagnostics.
pub fn live_backend_status() -> &'static str {
    "native Wayland/Cairo wl_shm menu adapter available via nuraloumi-menu --live"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shell_with(menu: MenuModel) -> ShellState {
        ShellState::new(menu, false).expect("valid test menu")
    }

    #[test]
    fn config_defaults_match_sl101_class() {
        let config = ShellConfig::default();
        assert_eq!(config.panel_height, 48);
        assert_eq!(config.menu_width, 480);
        assert_eq!(config.row_height, 52);
        assert_eq!(config.panel_edge, PanelEdge::Top);
        config.validate().expect("defaults valid");
    }

    #[test]
    fn config_rejects_invalid_geometry() {
        let invalid = ShellConfig {
            panel_height: 8,
            ..ShellConfig::default()
        };
        assert!(invalid.validate().is_err());
    }

    #[test]
    fn navigation_skips_status_and_wraps() {
        let mut shell = shell_with(build_launcher_menu());
        assert_eq!(shell.state.selected_id.as_deref(), Some("app.terminal"));
        shell.apply_semantic(SemanticInput::Up);
        assert_eq!(shell.state.selected_id.as_deref(), Some("app.music"));
        shell.apply_semantic(SemanticInput::Down);
        assert_eq!(shell.state.selected_id.as_deref(), Some("app.terminal"));
    }

    #[test]
    fn search_only_consumes_text_while_focused() {
        let mut shell = shell_with(build_launcher_menu());
        let ignored = shell.apply_semantic(SemanticInput::Text("term".into()));
        assert!(matches!(ignored, ActionReport::Ignored { .. }));
        shell.apply_input(ShellInput::SearchFocus(true));
        shell.apply_semantic(SemanticInput::Text("term".into()));
        assert_eq!(shell.state.query, "term");
        assert_eq!(shell.state.selected_id.as_deref(), Some("app.terminal"));
        shell.apply_semantic(SemanticInput::Back);
        assert!(!shell.search_focused);
        assert!(!shell.closed);
    }

    #[test]
    fn refresh_preserves_semantic_selection_when_possible() {
        let snapshot = FixtureSnapshot::default();
        let mut shell = shell_with(build_audio_menu(&snapshot));
        shell.apply_semantic(SemanticInput::Down);
        let selected = shell.state.selected_id.clone();

        let mut refreshed = snapshot;
        refreshed.clock.value = Some("23:43".into());
        shell
            .refresh_menu(build_audio_menu(&refreshed))
            .expect("refresh valid");
        assert_eq!(shell.state.selected_id, selected);
    }

    #[test]
    fn destructive_action_requires_second_activation() {
        let snapshot = FixtureSnapshot::default();
        let mut shell = shell_with(build_system_menu(&snapshot));
        shell.state.selected_id = Some("system.restart".into());

        let first = shell.apply_semantic(SemanticInput::Activate);
        assert!(matches!(first, ActionReport::ConfirmationRequired { .. }));
        assert_eq!(
            shell.pending_confirmation.as_deref(),
            Some("system.restart")
        );

        let second = shell.apply_semantic(SemanticInput::Activate);
        assert!(matches!(
            second,
            ActionReport::Dispatched {
                confirmed: true,
                action: MenuAction::Activate { ref id },
                ..
            } if id == "system.restart"
        ));
        assert!(shell.pending_confirmation.is_none());
    }

    #[test]
    fn escape_cancels_confirmation_before_closing() {
        let snapshot = FixtureSnapshot::default();
        let mut shell = shell_with(build_system_menu(&snapshot));
        shell.state.selected_id = Some("system.poweroff".into());
        shell.apply_semantic(SemanticInput::Activate);
        let report = shell.apply_semantic(SemanticInput::Back);
        assert!(matches!(
            report,
            ActionReport::ConfirmationCancelled { ref item_id }
                if item_id == "system.poweroff"
        ));
        assert!(!shell.closed);
    }

    #[test]
    fn touch_and_keyboard_share_dispatch_path() {
        let menu = build_launcher_menu();
        let mut keyboard = shell_with(menu.clone());
        let keyboard_report = keyboard.handle_platform_event(PlatformEvent::Key {
            key: PlatformKey::Enter,
            pressed: true,
        });

        let mut touch = shell_with(menu);
        let region = touch
            .hit_regions()
            .into_iter()
            .find(|region| region.item_id == "app.terminal")
            .expect("terminal hit region");
        assert!(touch
            .handle_platform_event(PlatformEvent::TouchDown {
                id: 7,
                region: Some(region.clone()),
            })
            .is_none());
        let touch_report = touch.handle_platform_event(PlatformEvent::TouchUp {
            id: 7,
            region: Some(region),
        });

        assert_eq!(keyboard_report, touch_report);
    }

    #[test]
    fn touch_release_outside_does_not_activate() {
        let mut shell = shell_with(build_launcher_menu());
        let terminal = HitRegion {
            region_id: "row:app.terminal".into(),
            item_id: "app.terminal".into(),
        };
        let files = HitRegion {
            region_id: "row:app.files".into(),
            item_id: "app.files".into(),
        };
        shell.handle_platform_event(PlatformEvent::TouchDown {
            id: 1,
            region: Some(terminal),
        });
        let report = shell
            .handle_platform_event(PlatformEvent::TouchUp {
                id: 1,
                region: Some(files),
            })
            .expect("release report");
        assert!(matches!(report, ActionReport::Ignored { .. }));
    }

    #[test]
    fn panel_has_no_focus_until_interactive_menu_opens() {
        let mut panel = PanelController::default();
        assert!(!panel.keyboard_focus);
        panel.open_menu(MenuFamily::Network, false);
        assert!(!panel.keyboard_focus);
        panel.open_menu(MenuFamily::Audio, true);
        assert_eq!(panel.active_menu, Some(MenuFamily::Audio));
        assert!(panel.keyboard_focus);
        panel.close_menu();
        assert!(!panel.keyboard_focus);
    }

    #[test]
    fn unavailable_provider_remains_visible_and_disabled() {
        let snapshot = FixtureSnapshot {
            network: ProviderValue {
                state: ValueState::Unavailable,
                value: None,
                message: Some("NetworkManager absent".into()),
            },
            ..FixtureSnapshot::default()
        };
        let menu = build_network_menu(&snapshot);
        assert!(menu.items.iter().any(|item| {
            item.id == "network.state"
                && item
                    .subtitle
                    .as_deref()
                    .is_some_and(|subtitle| subtitle.contains("Unavailable"))
        }));
        let toggle = menu
            .items
            .iter()
            .find(|item| item.id == "network.toggle")
            .expect("toggle row");
        assert!(!toggle.enabled);
        assert!(toggle.action.is_none());
    }

    #[test]
    fn control_family_aliases_parse_and_builtins_validate() {
        let snapshot = FixtureSnapshot::default();
        let cases = [
            ("wifi", MenuFamily::Network),
            ("bt", MenuFamily::Bluetooth),
            ("brightness", MenuFamily::Display),
            ("full-screen", MenuFamily::Display),
            ("speaker", MenuFamily::Audio),
            ("power", MenuFamily::Power),
            ("task-viewer", MenuFamily::Tasks),
            ("window-list", MenuFamily::Windows),
            ("controls", MenuFamily::System),
        ];

        for (alias, family) in cases {
            assert_eq!(parse_family(alias).expect("family alias"), family);
            build_family(family, &snapshot)
                .validate()
                .expect("built-in family validates");
        }
    }

    #[test]
    fn wifi_and_bluetooth_actions_degrade_with_provider_state() {
        let available = FixtureSnapshot::default();
        let wifi = build_network_menu(&available);
        assert!(wifi.items.iter().any(|item| {
            matches!(
                item.action,
                Some(MenuAction::Custom {
                    ref kind,
                    ref payload
                }) if kind == "network.connect" && payload == "Workshop"
            )
        }));

        let unavailable = FixtureSnapshot {
            network: ProviderValue {
                state: ValueState::Unavailable,
                value: None,
                message: Some("nmcli absent".into()),
            },
            bluetooth: ProviderValue {
                state: ValueState::Unavailable,
                value: None,
                message: Some("BlueZ absent".into()),
            },
            ..FixtureSnapshot::default()
        };
        let wifi = build_network_menu(&unavailable);
        let wifi_toggle = wifi
            .items
            .iter()
            .find(|item| item.id == "network.toggle")
            .expect("wifi toggle");
        assert!(!wifi_toggle.enabled);
        assert!(wifi_toggle.action.is_none());

        let bluetooth = build_bluetooth_menu(&unavailable);
        let bluetooth_toggle = bluetooth
            .items
            .iter()
            .find(|item| item.id == "bluetooth.toggle")
            .expect("bluetooth toggle");
        assert!(!bluetooth_toggle.enabled);
        assert!(bluetooth_toggle.action.is_none());
        assert!(bluetooth
            .items
            .iter()
            .filter(|item| item.id.starts_with("bluetooth.device."))
            .all(|item| !item.enabled && item.action.is_none()));
    }

    #[test]
    fn display_menu_exposes_brightness_and_focused_window_fullscreen() {
        let snapshot = FixtureSnapshot::default();
        let menu = build_display_menu(&snapshot);
        assert!(menu.items.iter().any(|item| {
            matches!(
                item.action,
                Some(MenuAction::Adjust {
                    ref id,
                    delta: -10
                }) if id == "system.brightness"
            )
        }));
        assert!(menu.items.iter().any(|item| {
            matches!(
                item.action,
                Some(MenuAction::Toggle { ref id })
                    if id == "window.fullscreen:terminal"
            )
        }));

        let read_only = FixtureSnapshot {
            brightness_writable: false,
            ..FixtureSnapshot::default()
        };
        let menu = build_display_menu(&read_only);
        assert!(menu
            .items
            .iter()
            .filter(
                |item| item.id == "display.brightness.down" || item.id == "display.brightness.up"
            )
            .all(|item| !item.enabled && item.action.is_none()));
    }

    #[test]
    fn power_and_window_close_require_confirmation() {
        let snapshot = FixtureSnapshot::default();

        let mut power = shell_with(build_power_menu(&snapshot));
        power.state.selected_id = Some("power.poweroff".into());
        assert!(matches!(
            power.apply_semantic(SemanticInput::Activate),
            ActionReport::ConfirmationRequired { .. }
        ));
        assert!(matches!(
            power.apply_semantic(SemanticInput::Activate),
            ActionReport::Dispatched {
                confirmed: true,
                action: MenuAction::Activate { ref id },
                ..
            } if id == "system.poweroff"
        ));

        let mut windows = shell_with(build_windows_menu(&snapshot));
        windows.state.selected_id = Some("windows.close".into());
        assert!(matches!(
            windows.apply_semantic(SemanticInput::Activate),
            ActionReport::ConfirmationRequired {
                action: MenuAction::Custom { ref kind, .. },
                ..
            } if kind == "window.close"
        ));
    }

    #[test]
    fn task_viewer_is_inspection_only_and_legacy_snapshot_shape_still_loads() {
        let tasks = build_tasks_menu(&FixtureSnapshot::default());
        assert!(tasks
            .items
            .iter()
            .filter_map(|item| item.action.as_ref())
            .all(|action| matches!(
                action,
                MenuAction::Custom { kind, .. } if kind == "task.inspect"
            )));

        let legacy = r#"{
            "network":{"state":"ready","value":"Lab","message":null},
            "audio":{"state":"ready","value":"40%","message":null},
            "battery":{"state":"ready","value":"70%","message":null},
            "clock":{"state":"ready","value":"12:00","message":null},
            "brightness":{"state":"ready","value":"50%","message":null}
        }"#;
        let snapshot: FixtureSnapshot =
            serde_json::from_str(legacy).expect("legacy fixture remains compatible");
        assert_eq!(snapshot.bluetooth.state, ValueState::Unavailable);
        assert!(snapshot.wifi_networks.is_empty());
        assert!(snapshot.tasks.is_empty());
        assert!(snapshot.windows.is_empty());
    }

    #[test]
    fn serde_json_fixture_round_trip() {
        let menu = build_launcher_menu();
        let encoded = serde_json::to_string(&menu).expect("serialize");
        let decoded: MenuModel = serde_json::from_str(&encoded).expect("deserialize");
        assert_eq!(menu, decoded);
    }

    #[test]
    fn serde_toml_config_round_trip() {
        let config = ShellConfig::default();
        let encoded = toml::to_string(&config).expect("serialize");
        let decoded: ShellConfig = toml::from_str(&encoded).expect("deserialize");
        assert_eq!(config, decoded);
    }
}
