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

mod window_adapter;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
pub use window_adapter::{
    execute_window_command, parse_window_command, window_entries, WindowCommand,
    WindowControlCapabilities,
};

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
            panel_height: 40,
            menu_width: 448,
            row_height: 48,
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
            ValueState::Unavailable => self.value.clone().unwrap_or_else(|| "Unavailable".into()),
            ValueState::Stale => self
                .value
                .as_deref()
                .map(|value| format!("Stale · {value}"))
                .unwrap_or_else(|| "Stale".into()),
            ValueState::Error => "Error".into(),
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

const fn default_window_control_enabled() -> bool {
    true
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
    #[serde(default = "default_window_control_enabled")]
    pub focusable: bool,
    #[serde(default = "default_window_control_enabled")]
    pub fullscreen_controllable: bool,
    #[serde(default = "default_window_control_enabled")]
    pub closable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplicationEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub generic_name: Option<String>,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub launchable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopEntry {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub urgent: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default = "default_desktop_switchable")]
    pub switchable: bool,
    #[serde(default)]
    pub window_count: usize,
}

const fn default_desktop_switchable() -> bool {
    true
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopControlCapabilities {
    pub list: bool,
    pub switch: bool,
    pub window_membership: bool,
    pub move_window: bool,
    pub sticky_window: bool,
}

impl DesktopControlCapabilities {
    pub const fn unavailable() -> Self {
        Self {
            list: false,
            switch: false,
            window_membership: false,
            move_window: false,
            sticky_window: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverviewMode {
    #[default]
    All,
    Windows,
    Apps,
    Desktops,
}

impl OverviewMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Windows => "windows",
            Self::Apps => "apps",
            Self::Desktops => "desktops",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "all" => Ok(Self::All),
            "windows" | "window" => Ok(Self::Windows),
            "apps" | "applications" => Ok(Self::Apps),
            "desktops" | "desktop" | "workspaces" | "workspace" => Ok(Self::Desktops),
            other => Err(format!("unknown overview mode {other:?}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct DesktopMovePayload {
    window_id: String,
    desktop_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DesktopCommand {
    Switch {
        desktop_id: String,
    },
    MoveWindow {
        window_id: String,
        desktop_id: String,
    },
}

pub fn parse_desktop_command(
    action: &MenuAction,
    snapshot: &FixtureSnapshot,
) -> Result<Option<DesktopCommand>, String> {
    match action {
        MenuAction::Custom { kind, payload } if kind == "desktop.switch" => {
            if !snapshot.desktop_capabilities.switch {
                return Err("compositor does not expose desktop switching".into());
            }
            let desktop = snapshot
                .desktops
                .iter()
                .find(|desktop| desktop.id == *payload)
                .ok_or_else(|| format!("desktop {payload:?} is not in the current snapshot"))?;
            if !desktop.switchable {
                return Err(format!("desktop {payload:?} is not activatable"));
            }
            Ok(Some(DesktopCommand::Switch {
                desktop_id: payload.clone(),
            }))
        }
        MenuAction::Custom { kind, payload } if kind == "desktop.move_window" => {
            if !snapshot.desktop_capabilities.move_window {
                return Err("compositor does not expose move-window-to-desktop control".into());
            }
            let payload: DesktopMovePayload = serde_json::from_str(payload)
                .map_err(|error| format!("invalid desktop move payload: {error}"))?;
            if !snapshot
                .windows
                .iter()
                .any(|window| window.id == payload.window_id)
            {
                return Err(format!(
                    "toplevel {:?} is not in the current snapshot",
                    payload.window_id
                ));
            }
            if !snapshot
                .desktops
                .iter()
                .any(|desktop| desktop.id == payload.desktop_id)
            {
                return Err(format!(
                    "desktop {:?} is not in the current snapshot",
                    payload.desktop_id
                ));
            }
            Ok(Some(DesktopCommand::MoveWindow {
                window_id: payload.window_id,
                desktop_id: payload.desktop_id,
            }))
        }
        _ => Ok(None),
    }
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
    #[serde(default)]
    pub applications: Vec<ApplicationEntry>,
    #[serde(default)]
    pub desktops: Vec<DesktopEntry>,
    #[serde(default)]
    pub desktop_capabilities: DesktopControlCapabilities,
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
                    focusable: true,
                    fullscreen_controllable: true,
                    closable: true,
                },
                WindowEntry {
                    id: "files".into(),
                    title: "Files".into(),
                    app_id: Some("thunar".into()),
                    focused: false,
                    fullscreen: false,
                    focusable: true,
                    fullscreen_controllable: true,
                    closable: true,
                },
            ],
            applications: vec![
                ApplicationEntry {
                    id: "foot.desktop".into(),
                    label: "Terminal".into(),
                    generic_name: Some("Terminal emulator".into()),
                    keywords: vec!["shell".into(), "console".into()],
                    launchable: true,
                },
                ApplicationEntry {
                    id: "thunar.desktop".into(),
                    label: "Files".into(),
                    generic_name: Some("File manager".into()),
                    keywords: vec!["files".into(), "folders".into()],
                    launchable: true,
                },
                ApplicationEntry {
                    id: "firefox.desktop".into(),
                    label: "Browser".into(),
                    generic_name: Some("Web browser".into()),
                    keywords: vec!["web".into(), "internet".into()],
                    launchable: true,
                },
            ],
            desktops: vec![
                DesktopEntry {
                    id: "1".into(),
                    label: "Desktop 1".into(),
                    active: true,
                    urgent: false,
                    hidden: false,
                    switchable: true,
                    window_count: 2,
                },
                DesktopEntry {
                    id: "2".into(),
                    label: "Desktop 2".into(),
                    active: false,
                    urgent: false,
                    hidden: false,
                    switchable: true,
                    window_count: 0,
                },
            ],
            desktop_capabilities: DesktopControlCapabilities {
                list: true,
                switch: false,
                window_membership: false,
                move_window: false,
                sticky_window: false,
            },
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
    ControlCenter,
    Network,
    Bluetooth,
    Display,
    Audio,
    Power,
    Tasks,
    Windows,
    System,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlCenterTab {
    #[default]
    Media,
    Network,
    Display,
    System,
    Notifications,
}

impl ControlCenterTab {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Media => "media",
            Self::Network => "network",
            Self::Display => "display",
            Self::System => "system",
            Self::Notifications => "notifications",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "media" | "audio" => Ok(Self::Media),
            "network" | "wifi" | "bluetooth" => Ok(Self::Network),
            "display" | "brightness" => Ok(Self::Display),
            "system" | "power" => Ok(Self::System),
            "notifications" | "notification" | "notices" => Ok(Self::Notifications),
            other => Err(format!("unknown control-center tab {other:?}")),
        }
    }
}

pub fn build_family(family: MenuFamily, snapshot: &FixtureSnapshot) -> MenuModel {
    match family {
        MenuFamily::Launcher => build_launcher_menu_for(snapshot, OverviewMode::All),
        MenuFamily::ControlCenter => {
            build_control_center_menu(snapshot, ControlCenterTab::default())
        }
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
    build_launcher_menu_for(&FixtureSnapshot::default(), OverviewMode::All)
}

pub fn build_launcher_menu_for(snapshot: &FixtureSnapshot, mode: OverviewMode) -> MenuModel {
    let mut items = vec![
        status(
            "launcher.search",
            "Search windows, apps and desktops…",
            "Type while search is focused",
        ),
        section("launcher.views", "Overview"),
    ];

    for candidate in [
        OverviewMode::All,
        OverviewMode::Windows,
        OverviewMode::Apps,
        OverviewMode::Desktops,
    ] {
        let selected = candidate == mode;
        let label = match candidate {
            OverviewMode::All => "All",
            OverviewMode::Windows => "Windows",
            OverviewMode::Apps => "Apps",
            OverviewMode::Desktops => "Desktops",
        };
        items.push(custom_action(
            &format!("overview.mode.{}", candidate.as_str()),
            label,
            Some(if selected {
                "Selected view"
            } else {
                "Switch view"
            }),
            "overview.mode",
            candidate.as_str(),
            !selected,
        ));
    }

    if mode == OverviewMode::All {
        items.extend([
            section("launcher.controls", "Desktop"),
            custom_action(
                "launcher.control-center",
                "Control Center",
                Some("Media · network · display · system · notifications"),
                "menu.open",
                "control-center",
                true,
            ),
        ]);
    }

    if matches!(mode, OverviewMode::All | OverviewMode::Windows) {
        items.push(section("launcher.windows", "Open windows"));
        if snapshot.windows.is_empty() {
            items.push(status(
                "launcher.windows.empty",
                "No open windows",
                "Compositor reported no mapped toplevels",
            ));
        } else {
            items.extend(snapshot.windows.iter().take(16).map(|window| {
                let subtitle = match (&window.app_id, window.focused) {
                    (Some(app_id), true) => format!("Focused · {app_id}"),
                    (Some(app_id), false) => app_id.clone(),
                    (None, true) => "Focused".to_owned(),
                    (None, false) => "Open window".to_owned(),
                };
                custom_action(
                    &format!("overview.window.{}", window.id),
                    &window.title,
                    Some(&subtitle),
                    "window.focus",
                    &window.id,
                    window.focusable,
                )
            }));
        }
    }

    if matches!(mode, OverviewMode::All | OverviewMode::Apps) {
        items.push(section("launcher.apps", "Applications"));
        if snapshot.applications.is_empty() {
            items.push(status(
                "launcher.apps.empty",
                "No applications",
                "Desktop-entry provider has no visible applications",
            ));
        } else {
            items.extend(snapshot.applications.iter().take(32).map(|application| {
                let mut detail = Vec::new();
                if let Some(generic_name) = &application.generic_name {
                    detail.push(generic_name.clone());
                }
                if !application.keywords.is_empty() {
                    detail.push(
                        application
                            .keywords
                            .iter()
                            .take(3)
                            .cloned()
                            .collect::<Vec<_>>()
                            .join(" · "),
                    );
                }
                let subtitle = if detail.is_empty() {
                    Some(application.id.as_str())
                } else {
                    None
                };
                let joined_detail = (!detail.is_empty()).then(|| detail.join(" · "));
                custom_action(
                    &format!("overview.app.{}", application.id),
                    &application.label,
                    joined_detail.as_deref().or(subtitle),
                    "app.launch",
                    &application.id,
                    application.launchable,
                )
            }));
        }
    }

    if matches!(mode, OverviewMode::All | OverviewMode::Desktops) {
        items.push(section("launcher.desktops", "Desktops"));
        if !snapshot.desktop_capabilities.list || snapshot.desktops.is_empty() {
            items.push(status(
                "launcher.desktops.unavailable",
                "Desktop integration unavailable",
                "Compositor adapter has not exposed desktop/workspace listing",
            ));
        } else {
            items.extend(
                snapshot
                    .desktops
                    .iter()
                    .filter(|desktop| !desktop.hidden)
                    .take(16)
                    .map(|desktop| {
                        let subtitle = match (
                            desktop.active,
                            desktop.urgent,
                            snapshot.desktop_capabilities.window_membership,
                        ) {
                            (true, true, true) => {
                                format!(
                                    "Active · needs attention · {} windows",
                                    desktop.window_count
                                )
                            }
                            (true, false, true) => {
                                format!("Active · {} windows", desktop.window_count)
                            }
                            (false, true, true) => {
                                format!("Needs attention · {} windows", desktop.window_count)
                            }
                            (false, false, true) => format!("{} windows", desktop.window_count),
                            (true, true, false) => "Active · needs attention".to_owned(),
                            (true, false, false) => "Active".to_owned(),
                            (false, true, false) => "Needs attention".to_owned(),
                            (false, false, false) => "Workspace".to_owned(),
                        };
                        if desktop.active {
                            status(
                                &format!("overview.desktop.{}", desktop.id),
                                &desktop.label,
                                &subtitle,
                            )
                        } else {
                            custom_action(
                                &format!("overview.desktop.{}", desktop.id),
                                &desktop.label,
                                Some(&subtitle),
                                "desktop.switch",
                                &desktop.id,
                                snapshot.desktop_capabilities.switch && desktop.switchable,
                            )
                        }
                    }),
            );
        }

        if mode == OverviewMode::Desktops {
            if let Some(window) = snapshot.windows.iter().find(|window| window.focused) {
                items.push(section("launcher.desktop.move", "Move active window"));
                for desktop in snapshot.desktops.iter().take(16) {
                    let payload = serde_json::to_string(&DesktopMovePayload {
                        window_id: window.id.clone(),
                        desktop_id: desktop.id.clone(),
                    })
                    .expect("desktop move payload is serializable");
                    items.push(custom_action(
                        &format!("overview.move.{}.{}", window.id, desktop.id),
                        &desktop.label,
                        Some(&window.title),
                        "desktop.move_window",
                        &payload,
                        snapshot.desktop_capabilities.move_window,
                    ));
                }
            }
        }
    }

    MenuModel {
        id: "launcher".into(),
        title: "Overview".into(),
        items,
    }
}

pub fn build_control_center_menu(snapshot: &FixtureSnapshot, tab: ControlCenterTab) -> MenuModel {
    let mut items = vec![section("control.tabs", "Control Center")];

    for candidate in [
        ControlCenterTab::Media,
        ControlCenterTab::Network,
        ControlCenterTab::Display,
        ControlCenterTab::System,
        ControlCenterTab::Notifications,
    ] {
        let selected = candidate == tab;
        let label = match candidate {
            ControlCenterTab::Media => "Media",
            ControlCenterTab::Network => "Network",
            ControlCenterTab::Display => "Display",
            ControlCenterTab::System => "System",
            ControlCenterTab::Notifications => "Notifications",
        };
        items.push(custom_action(
            &format!("control.tab.{}", candidate.as_str()),
            label,
            Some(if selected {
                "Selected tab"
            } else {
                "Switch tab"
            }),
            "control.tab",
            candidate.as_str(),
            !selected,
        ));
    }

    match tab {
        ControlCenterTab::Media => {
            let available = snapshot.audio.available();
            items.extend([
                section("control.media.now-playing", "Now playing"),
                status(
                    "control.media.player",
                    "Media player",
                    "MPRIS provider not connected yet",
                ),
                section("control.media.audio", "Audio"),
                provider_status("control.media.volume", "Speaker volume", &snapshot.audio),
                toggle_action(
                    "control.media.mute",
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
                adjustable(
                    "control.media.down",
                    "Volume −5%",
                    "audio.volume",
                    -5,
                    available,
                ),
                adjustable(
                    "control.media.up",
                    "Volume +5%",
                    "audio.volume",
                    5,
                    available,
                ),
            ]);
        }
        ControlCenterTab::Network => {
            let wifi_available = snapshot.network.available();
            let bluetooth_available = snapshot.bluetooth.available();
            items.extend([
                section("control.network.status", "Connectivity"),
                provider_status("control.network.wifi.state", "Wi-Fi", &snapshot.network),
                toggle_action(
                    "control.network.wifi.toggle",
                    "Wi-Fi radio",
                    "network.wifi",
                    wifi_available,
                    None,
                    wifi_available.then_some("Toggle radio through the network provider"),
                ),
                custom_action(
                    "control.network.wifi.details",
                    "Wi-Fi networks…",
                    Some("Open scan results and connection controls"),
                    "menu.open",
                    "wifi",
                    true,
                ),
                provider_status(
                    "control.network.bluetooth.state",
                    "Bluetooth",
                    &snapshot.bluetooth,
                ),
                toggle_action(
                    "control.network.bluetooth.toggle",
                    "Bluetooth radio",
                    "bluetooth.radio",
                    bluetooth_available,
                    snapshot
                        .bluetooth
                        .value
                        .as_deref()
                        .map(|value| value.trim_start().starts_with("On")),
                    bluetooth_available.then_some("Toggle adapter power"),
                ),
                custom_action(
                    "control.network.bluetooth.details",
                    "Bluetooth devices…",
                    Some("Open paired and known device controls"),
                    "menu.open",
                    "bluetooth",
                    true,
                ),
            ]);
        }
        ControlCenterTab::Display => {
            let available = snapshot.brightness.available() && snapshot.brightness_writable;
            items.extend([
                section("control.display.brightness", "Display"),
                provider_status(
                    "control.display.brightness.state",
                    "Brightness",
                    &snapshot.brightness,
                ),
                adjustable(
                    "control.display.brightness.down",
                    "Brightness −10%",
                    "system.brightness",
                    -10,
                    available,
                ),
                adjustable(
                    "control.display.brightness.up",
                    "Brightness +10%",
                    "system.brightness",
                    10,
                    available,
                ),
                custom_action(
                    "control.display.details",
                    "Display controls…",
                    Some("Brightness and active-window fullscreen"),
                    "menu.open",
                    "display",
                    true,
                ),
            ]);
        }
        ControlCenterTab::System => {
            items.extend([
                section("control.system.status", "System"),
                provider_status("control.system.battery", "Battery", &snapshot.battery),
                provider_status("control.system.clock", "Clock", &snapshot.clock),
                section("control.system.session", "Session"),
                confirm_action("control.system.suspend", "Suspend", "system.suspend"),
                confirm_action("control.system.restart", "Restart…", "system.restart"),
                confirm_action("control.system.poweroff", "Power off…", "system.poweroff"),
            ]);
        }
        ControlCenterTab::Notifications => {
            items.extend([
                section("control.notifications.center", "Notifications"),
                status(
                    "control.notifications.unavailable",
                    "No notification center provider",
                    "History and actions will appear here when notification integration is connected",
                ),
                status(
                    "control.notifications.empty",
                    "No notifications",
                    "Stable empty state",
                ),
            ]);
        }
    }

    MenuModel {
        id: "control-center".into(),
        title: "Control Center".into(),
        items,
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
            window.fullscreen_controllable,
            Some(window.fullscreen),
            Some(if !window.fullscreen_controllable {
                "Compositor exposes a read-only window list"
            } else if window.fullscreen {
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
    let has_windows = !snapshot.windows.is_empty();
    let has_controls = snapshot
        .windows
        .iter()
        .any(|window| window.focusable || window.fullscreen_controllable || window.closable);
    let source_status = if !has_windows {
        "No mapped compositor windows"
    } else if has_controls {
        "Compositor list and control available"
    } else {
        "Compositor list available · controls unavailable"
    };
    let mut items = vec![
        section("windows.summary", "Windows"),
        status("windows.source", "Window source", source_status),
    ];

    if let Some(window) = snapshot.windows.iter().find(|window| window.focused) {
        items.push(toggle_action(
            "windows.fullscreen",
            "Toggle fullscreen",
            &format!("window.fullscreen:{}", window.id),
            window.fullscreen_controllable,
            Some(window.fullscreen),
            Some(&window.title),
        ));
        items.push(confirm_custom_action(
            "windows.close",
            "Close active window…",
            Some(&window.title),
            "window.close",
            &window.id,
            window.closable,
        ));
    }

    items.push(section("windows.list", "Open windows"));
    if snapshot.windows.is_empty() {
        items.push(status(
            "windows.empty",
            "No windows",
            "Compositor reported no mapped toplevels or no supported protocol is available",
        ));
    } else {
        items.extend(snapshot.windows.iter().take(32).map(window_list_item));
    }

    MenuModel {
        id: "windows".into(),
        title: "Windows".into(),
        items,
    }
}

fn window_list_item(window: &WindowEntry) -> MenuItem {
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
    let item_id = format!("windows.item.{}", window.id);
    let has_actionable_control =
        (!window.focused && window.focusable) || window.fullscreen_controllable || window.closable;

    if !has_actionable_control {
        return status(
            &item_id,
            &window.title,
            &format!("{subtitle} · controls unavailable"),
        );
    }

    let focus_id = format!("{item_id}.focus");
    let fullscreen_id = format!("{item_id}.fullscreen");
    let close_id = format!("{item_id}.close");
    let mut children = Vec::with_capacity(3);

    if window.focused {
        children.push(status(&focus_id, "Focus", "Focused"));
    } else {
        children.push(custom_action(
            &focus_id,
            "Focus",
            Some("Activate this window"),
            "window.focus",
            &window.id,
            window.focusable,
        ));
    }

    children.push(toggle_action(
        &fullscreen_id,
        "Fullscreen",
        &format!("window.fullscreen:{}", window.id),
        window.fullscreen_controllable,
        Some(window.fullscreen),
        Some(if window.fullscreen {
            "Currently fullscreen"
        } else {
            "Currently windowed"
        }),
    ));
    children.push(confirm_custom_action(
        &close_id,
        "Close…",
        Some(&window.title),
        "window.close",
        &window.id,
        window.closable,
    ));

    let mut item = MenuItem::submenu(item_id, &window.title, children);
    item.subtitle = Some(subtitle);
    item
}

pub fn build_system_menu(snapshot: &FixtureSnapshot) -> MenuModel {
    let mut items = vec![
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
    ];

    if !snapshot.tasks.is_empty() {
        items.push(custom_action(
            "system.open.tasks",
            "Tasks",
            Some("Running tasks"),
            "menu.open",
            "tasks",
            true,
        ));
    }
    if !snapshot.windows.is_empty() {
        items.push(custom_action(
            "system.open.windows",
            "Windows",
            Some("Open windows"),
            "menu.open",
            "windows",
            true,
        ));
    }

    items.extend([
        section("system.session", "Power"),
        confirm_action("system.suspend", "Suspend", "system.suspend"),
        confirm_action("system.restart", "Restart…", "system.restart"),
        confirm_action("system.poweroff", "Power off…", "system.poweroff"),
    ]);

    MenuModel {
        id: "system".into(),
        title: "System".into(),
        items,
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

pub fn launcher_search_input() -> ShellInput {
    ShellInput::SearchFocus(true)
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
    #[serde(default)]
    pub overview_mode: OverviewMode,
    #[serde(default)]
    pub control_center_tab: ControlCenterTab,
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
            overview_mode: OverviewMode::All,
            control_center_tab: ControlCenterTab::Media,
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

    pub fn refresh_family(
        &mut self,
        family: MenuFamily,
        snapshot: &FixtureSnapshot,
    ) -> Result<(), String> {
        let menu = match family {
            MenuFamily::Launcher => build_launcher_menu_for(snapshot, self.overview_mode),
            MenuFamily::ControlCenter => {
                build_control_center_menu(snapshot, self.control_center_tab)
            }
            _ => build_family(family, snapshot),
        };
        self.refresh_menu(menu)
    }

    pub fn set_overview_mode(
        &mut self,
        mode: OverviewMode,
        snapshot: &FixtureSnapshot,
    ) -> Result<(), String> {
        if self.menu.id != "launcher" {
            return Err("overview mode is only valid for the launcher surface".into());
        }
        self.overview_mode = mode;
        self.refresh_menu(build_launcher_menu_for(snapshot, mode))
    }

    pub fn set_control_center_tab(
        &mut self,
        tab: ControlCenterTab,
        snapshot: &FixtureSnapshot,
    ) -> Result<(), String> {
        if self.menu.id != "control-center" {
            return Err("control-center tab is only valid for the Control Center surface".into());
        }
        self.control_center_tab = tab;
        self.refresh_menu(build_control_center_menu(snapshot, tab))
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
                self.state.query.clear();
                self.state.normalize(&self.menu);
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
        "launcher" | "apps" | "overview" | "super-menu" => Ok(MenuFamily::Launcher),
        "control-center" | "control-center-menu" | "quick-settings" => {
            Ok(MenuFamily::ControlCenter)
        }
        "network" | "net" | "wifi" | "wi-fi" => Ok(MenuFamily::Network),
        "bluetooth" | "bt" => Ok(MenuFamily::Bluetooth),
        "display" | "brightness" | "fullscreen" | "full-screen" => Ok(MenuFamily::Display),
        "audio" | "speaker" | "sound" | "volume" => Ok(MenuFamily::Audio),
        "power" | "session" => Ok(MenuFamily::Power),
        "tasks" | "task" | "task-viewer" => Ok(MenuFamily::Tasks),
        "windows" | "window" | "window-list" => Ok(MenuFamily::Windows),
        "system" | "battery" | "clock" | "controls" => Ok(MenuFamily::System),
        other => Err(format!(
            "unknown menu family {other:?}; expected launcher, control-center, wifi, bluetooth, display, audio, power, tasks, windows, or system"
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
        assert_eq!(config.panel_height, 40);
        assert_eq!(config.menu_width, 448);
        assert_eq!(config.row_height, 48);
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
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("overview.mode.windows")
        );
        shell.apply_semantic(SemanticInput::Up);
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("overview.app.firefox.desktop")
        );
        shell.apply_semantic(SemanticInput::Down);
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("overview.mode.windows")
        );
    }

    #[test]
    fn search_only_consumes_text_while_focused() {
        let mut shell = shell_with(build_launcher_menu());
        let ignored = shell.apply_semantic(SemanticInput::Text("term".into()));
        assert!(matches!(ignored, ActionReport::Ignored { .. }));
        shell.apply_input(launcher_search_input());
        shell.apply_semantic(SemanticInput::Text("term".into()));
        assert_eq!(shell.state.query, "term");
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("overview.window.terminal")
        );
    }

    #[test]
    fn super_menu_all_view_unifies_windows_apps_and_desktops() {
        let snapshot = FixtureSnapshot::default();
        let menu = build_launcher_menu_for(&snapshot, OverviewMode::All);
        assert_eq!(menu.title, "Overview");
        assert!(menu
            .items
            .iter()
            .any(|item| item.id == "overview.window.terminal"));
        let app = menu
            .items
            .iter()
            .find(|item| item.id == "overview.app.foot.desktop")
            .expect("desktop application");
        assert!(matches!(
            app.action,
            Some(MenuAction::Custom {
                ref kind,
                ref payload
            }) if kind == "app.launch" && payload == "foot.desktop"
        ));
        assert!(menu
            .items
            .iter()
            .any(|item| item.id == "overview.desktop.1"));
    }

    #[test]
    fn overview_modes_share_one_canonical_search_state() {
        let snapshot = FixtureSnapshot::default();
        let mut shell = shell_with(build_launcher_menu_for(&snapshot, OverviewMode::All));
        shell.apply_input(launcher_search_input());
        shell.apply_semantic(SemanticInput::Text("term".into()));
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("overview.window.terminal")
        );

        shell
            .set_overview_mode(OverviewMode::Apps, &snapshot)
            .expect("switch app view");
        assert_eq!(shell.state.query, "term");
        assert!(shell.search_focused);
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("overview.app.foot.desktop")
        );

        shell
            .set_overview_mode(OverviewMode::Windows, &snapshot)
            .expect("switch window view");
        assert_eq!(shell.state.query, "term");
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("overview.window.terminal")
        );
    }

    #[test]
    fn desktop_controls_fail_closed_until_adapter_capabilities_exist() {
        let mut snapshot = FixtureSnapshot::default();
        let switch = MenuAction::Custom {
            kind: "desktop.switch".into(),
            payload: "2".into(),
        };
        assert!(parse_desktop_command(&switch, &snapshot).is_err());

        snapshot.desktop_capabilities.switch = true;
        assert_eq!(
            parse_desktop_command(&switch, &snapshot).unwrap(),
            Some(DesktopCommand::Switch {
                desktop_id: "2".into()
            })
        );

        snapshot.desktops[1].switchable = false;
        assert!(parse_desktop_command(&switch, &snapshot).is_err());
        snapshot.desktops[1].switchable = true;

        let display_name = MenuAction::Custom {
            kind: "desktop.switch".into(),
            payload: snapshot.desktops[1].label.clone(),
        };
        assert!(parse_desktop_command(&display_name, &snapshot).is_err());

        let move_window = MenuAction::Custom {
            kind: "desktop.move_window".into(),
            payload: serde_json::to_string(&DesktopMovePayload {
                window_id: "terminal".into(),
                desktop_id: "2".into(),
            })
            .unwrap(),
        };
        assert!(parse_desktop_command(&move_window, &snapshot).is_err());
        snapshot.desktop_capabilities.move_window = true;
        assert_eq!(
            parse_desktop_command(&move_window, &snapshot).unwrap(),
            Some(DesktopCommand::MoveWindow {
                window_id: "terminal".into(),
                desktop_id: "2".into(),
            })
        );
    }

    #[test]
    fn desktop_overview_hides_hidden_workspaces_without_claiming_membership() {
        let mut snapshot = FixtureSnapshot::default();
        snapshot.desktop_capabilities.list = true;
        snapshot.desktop_capabilities.switch = true;
        snapshot.desktop_capabilities.window_membership = false;
        snapshot.desktops[0].hidden = true;
        snapshot.desktops[1].urgent = true;

        let menu = build_launcher_menu_for(&snapshot, OverviewMode::Desktops);
        assert!(!menu
            .items
            .iter()
            .any(|item| item.id == "overview.desktop.1"));

        let second = menu
            .items
            .iter()
            .find(|item| item.id == "overview.desktop.2")
            .expect("visible workspace");
        assert_eq!(second.subtitle.as_deref(), Some("Needs attention"));
        assert!(matches!(
            second.action,
            Some(MenuAction::Custom {
                ref kind,
                ref payload
            }) if kind == "desktop.switch" && payload == "2"
        ));
    }

    #[test]
    fn back_and_escape_clear_search_before_close_and_preserve_selection() {
        for input in [SemanticInput::Back, SemanticInput::Escape] {
            let mut shell = shell_with(build_launcher_menu());
            shell.apply_input(launcher_search_input());
            shell.apply_semantic(SemanticInput::Text("term".into()));
            let selected = shell.state.selected_id.clone();

            assert_eq!(
                shell.apply_semantic(input.clone()),
                ActionReport::SearchChanged {
                    query: String::new(),
                    focused: false,
                }
            );
            assert!(shell.state.query.is_empty());
            assert_eq!(shell.state.selected_id, selected);
            assert!(!shell.search_focused);
            assert!(!shell.closed);

            assert_eq!(shell.apply_semantic(input), ActionReport::SurfaceClosed);
            assert!(shell.closed);
        }
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
    fn live_system_menu_omits_unpopulated_conceptual_collections() {
        let mut snapshot = FixtureSnapshot::default();
        snapshot.tasks.clear();
        snapshot.windows.clear();
        let menu = build_system_menu(&snapshot);
        assert!(!menu.items.iter().any(|item| item.id == "system.open.tasks"));
        assert!(!menu
            .items
            .iter()
            .any(|item| item.id == "system.open.windows"));

        snapshot.tasks.push(TaskEntry {
            id: "labwc".into(),
            label: "labwc".into(),
            state: "Running".into(),
            cpu_percent: None,
            memory_mib: None,
        });
        snapshot.windows.push(WindowEntry {
            id: "terminal".into(),
            title: "Terminal".into(),
            app_id: Some("foot".into()),
            focused: true,
            fullscreen: false,
            focusable: true,
            fullscreen_controllable: true,
            closable: true,
        });
        let menu = build_system_menu(&snapshot);
        assert!(menu.items.iter().any(|item| item.id == "system.open.tasks"));
        assert!(menu
            .items
            .iter()
            .any(|item| item.id == "system.open.windows"));
    }

    #[test]
    fn provider_status_suppresses_command_diagnostics_in_primary_ui() {
        let unavailable = ProviderValue {
            state: ValueState::Unavailable,
            value: None,
            message: Some("wpctl exited 127: command not found".into()),
        };
        assert_eq!(unavailable.subtitle(), "Unavailable");

        let error = ProviderValue {
            state: ValueState::Error,
            value: None,
            message: Some("backend-specific diagnostics".into()),
        };
        assert_eq!(error.subtitle(), "Error");
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
        keyboard.state.selected_id = Some("overview.app.foot.desktop".into());
        let keyboard_report = keyboard.handle_platform_event(PlatformEvent::Key {
            key: PlatformKey::Enter,
            pressed: true,
        });

        let mut touch = shell_with(menu);
        let region = touch
            .hit_regions()
            .into_iter()
            .find(|region| region.item_id == "overview.app.foot.desktop")
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
            ("super-menu", MenuFamily::Launcher),
            ("quick-settings", MenuFamily::ControlCenter),
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
    fn control_center_tabs_reuse_existing_provider_actions_and_degrade_cleanly() {
        let snapshot = FixtureSnapshot::default();

        let media = build_control_center_menu(&snapshot, ControlCenterTab::Media);
        assert_eq!(media.id, "control-center");
        assert!(media.items.iter().any(|item| {
            matches!(
                item.action,
                Some(MenuAction::Adjust {
                    ref id,
                    delta: 5
                }) if id == "audio.volume"
            )
        }));
        assert!(media.items.iter().any(|item| {
            item.id == "control.media.player"
                && item
                    .subtitle
                    .as_deref()
                    .is_some_and(|subtitle| subtitle.contains("MPRIS"))
        }));

        let network = build_control_center_menu(&snapshot, ControlCenterTab::Network);
        assert!(network.items.iter().any(|item| {
            matches!(
                item.action,
                Some(MenuAction::Toggle { ref id }) if id == "network.wifi"
            )
        }));
        assert!(network.items.iter().any(|item| {
            matches!(
                item.action,
                Some(MenuAction::Toggle { ref id }) if id == "bluetooth.radio"
            )
        }));

        let notifications = build_control_center_menu(&snapshot, ControlCenterTab::Notifications);
        assert!(notifications
            .items
            .iter()
            .any(|item| item.id == "control.notifications.unavailable"));

        for tab in [
            ControlCenterTab::Media,
            ControlCenterTab::Network,
            ControlCenterTab::Display,
            ControlCenterTab::System,
            ControlCenterTab::Notifications,
        ] {
            build_control_center_menu(&snapshot, tab)
                .validate()
                .expect("control-center tab validates");
        }
    }

    #[test]
    fn control_center_tab_switch_rebuilds_without_parallel_action_path() {
        let snapshot = FixtureSnapshot::default();
        let mut shell = shell_with(build_control_center_menu(
            &snapshot,
            ControlCenterTab::Media,
        ));
        shell
            .set_control_center_tab(ControlCenterTab::Display, &snapshot)
            .expect("switch control-center tab");
        assert_eq!(shell.control_center_tab, ControlCenterTab::Display);
        assert_eq!(shell.menu.id, "control-center");
        assert!(shell
            .menu
            .items
            .iter()
            .any(|item| item.id == "control.display.brightness.up"));
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
    fn window_controls_degrade_to_list_only() {
        let mut snapshot = FixtureSnapshot::default();
        for window in &mut snapshot.windows {
            window.focusable = false;
            window.fullscreen_controllable = false;
            window.closable = false;
        }
        let windows = build_windows_menu(&snapshot);
        let focus = windows
            .items
            .iter()
            .find(|item| item.id == "windows.item.terminal")
            .expect("window row");
        assert!(!focus.enabled);
        assert!(focus.action.is_none());
        let close = windows
            .items
            .iter()
            .find(|item| item.id == "windows.close")
            .expect("close row");
        assert!(!close.enabled);
        assert!(close.action.is_none());
        let display = build_display_menu(&snapshot);
        let fullscreen = display
            .items
            .iter()
            .find(|item| item.id == "display.fullscreen")
            .expect("fullscreen row");
        assert!(!fullscreen.enabled);
        assert!(fullscreen.action.is_none());
    }

    #[test]
    fn unfocused_window_submenu_exposes_bounded_controls() {
        let snapshot = FixtureSnapshot {
            windows: vec![WindowEntry {
                id: "tl:0000000000000001".into(),
                title: "Test window".into(),
                app_id: Some("test.app".into()),
                focused: false,
                fullscreen: false,
                focusable: true,
                fullscreen_controllable: true,
                closable: true,
            }],
            ..FixtureSnapshot::default()
        };

        let mut shell = shell_with(build_windows_menu(&snapshot));
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("windows.item.tl:0000000000000001")
        );
        assert!(matches!(
            shell.apply_semantic(SemanticInput::Activate),
            ActionReport::NavigationChanged { .. }
        ));
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("windows.item.tl:0000000000000001.focus")
        );
        let _ = shell.apply_semantic(SemanticInput::Down);
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("windows.item.tl:0000000000000001.fullscreen")
        );
        let _ = shell.apply_semantic(SemanticInput::Down);
        assert_eq!(
            shell.state.selected_id.as_deref(),
            Some("windows.item.tl:0000000000000001.close")
        );
        assert!(matches!(
            shell.apply_semantic(SemanticInput::Activate),
            ActionReport::ConfirmationRequired {
                action: MenuAction::Custom { ref kind, ref payload },
                ..
            } if kind == "window.close" && payload == "tl:0000000000000001"
        ));
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
