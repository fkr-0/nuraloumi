use crate::{ActionReport, ControlCenterTab, MenuAction, OverviewMode, SemanticInput, ShellState};
use nuraloumi_wayland::{Key as WaylandKey, MediaKey, Modifiers as WaylandModifiers};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

const MAX_USER_BINDINGS: usize = 128;
const MAX_BINDING_ID_BYTES: usize = 128;
const MAX_KEY_NOTATION_BYTES: usize = 96;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct KeybindingConfig {
    pub enabled: bool,
    pub bindings: Vec<BindingOverride>,
}

impl Default for KeybindingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bindings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BindingOverride {
    pub id: String,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub keys: Option<String>,
    #[serde(default)]
    pub scope: Option<BindingScope>,
    #[serde(default)]
    pub action: Option<ActionSpec>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BindingScope {
    Global,
    Panel,
    Menu,
    Launcher,
    ControlCenter,
    Windows,
    Applications,
    Tasks,
    Desktops,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSpec {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tab: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<i32>,
}

impl ActionSpec {
    fn simple(id: &str) -> Self {
        Self {
            id: id.to_owned(),
            direction: None,
            family: None,
            mode: None,
            tab: None,
            delta: None,
        }
    }

    fn with_direction(direction: &str) -> Self {
        Self {
            id: "menu.navigate".into(),
            direction: Some(direction.into()),
            ..Self::simple("menu.navigate")
        }
    }

    fn with_delta(id: &str, delta: i32) -> Self {
        Self {
            id: id.into(),
            delta: Some(delta),
            ..Self::simple(id)
        }
    }

    fn resolve(&self) -> Result<BindingDispatchAction, String> {
        match self.id.as_str() {
            "menu.navigate" => {
                self.require_only(&["direction"])?;
                let input = match self.direction.as_deref() {
                    Some("up") => SemanticInput::Up,
                    Some("down") => SemanticInput::Down,
                    Some("left") => SemanticInput::Left,
                    Some("right") => SemanticInput::Right,
                    Some(other) => {
                        return Err(format!(
                            "action menu.navigate has invalid direction {other:?}"
                        ))
                    }
                    None => return Err("action menu.navigate requires direction".into()),
                };
                Ok(BindingDispatchAction::Semantic(input))
            }
            "menu.activate" => {
                self.require_only(&[])?;
                Ok(BindingDispatchAction::Semantic(SemanticInput::Activate))
            }
            "menu.back-or-close" => {
                self.require_only(&[])?;
                Ok(BindingDispatchAction::Semantic(SemanticInput::Back))
            }
            "menu.backspace" => {
                self.require_only(&[])?;
                Ok(BindingDispatchAction::Semantic(SemanticInput::Backspace))
            }
            "menu.open" => {
                self.require_only(&["family"])?;
                let family = self
                    .family
                    .as_deref()
                    .ok_or_else(|| "action menu.open requires family".to_owned())?;
                crate::parse_family(family)?;
                Ok(BindingDispatchAction::Menu(MenuAction::Custom {
                    kind: "menu.open".into(),
                    payload: family.to_owned(),
                }))
            }
            "overview.mode" => {
                self.require_only(&["mode"])?;
                let mode = self
                    .mode
                    .as_deref()
                    .ok_or_else(|| "action overview.mode requires mode".to_owned())?;
                OverviewMode::parse(mode)?;
                Ok(BindingDispatchAction::Menu(MenuAction::Custom {
                    kind: "overview.mode".into(),
                    payload: mode.to_owned(),
                }))
            }
            "control.tab" => {
                self.require_only(&["tab"])?;
                let tab = self
                    .tab
                    .as_deref()
                    .ok_or_else(|| "action control.tab requires tab".to_owned())?;
                ControlCenterTab::parse(tab)?;
                Ok(BindingDispatchAction::Menu(MenuAction::Custom {
                    kind: "control.tab".into(),
                    payload: tab.to_owned(),
                }))
            }
            "audio.adjust" => {
                self.require_only(&["delta"])?;
                let delta = bounded_delta(self.delta, "audio.adjust", 20)?;
                Ok(BindingDispatchAction::Menu(MenuAction::Adjust {
                    id: "audio.volume".into(),
                    delta,
                }))
            }
            "audio.toggle-mute" => {
                self.require_only(&[])?;
                Ok(BindingDispatchAction::Menu(MenuAction::Toggle {
                    id: "audio.mute".into(),
                }))
            }
            "display.adjust" => {
                self.require_only(&["delta"])?;
                let delta = bounded_delta(self.delta, "display.adjust", 20)?;
                Ok(BindingDispatchAction::Menu(MenuAction::Adjust {
                    id: "system.brightness".into(),
                    delta,
                }))
            }
            "network.toggle-wifi" => {
                self.require_only(&[])?;
                Ok(BindingDispatchAction::Menu(MenuAction::Toggle {
                    id: "network.wifi".into(),
                }))
            }
            "bluetooth.toggle-radio" => {
                self.require_only(&[])?;
                Ok(BindingDispatchAction::Menu(MenuAction::Toggle {
                    id: "bluetooth.radio".into(),
                }))
            }
            other => Err(format!("unknown keybinding action id {other:?}")),
        }
    }

    fn require_only(&self, allowed: &[&str]) -> Result<(), String> {
        for (name, present) in [
            ("direction", self.direction.is_some()),
            ("family", self.family.is_some()),
            ("mode", self.mode.is_some()),
            ("tab", self.tab.is_some()),
            ("delta", self.delta.is_some()),
        ] {
            if present && !allowed.contains(&name) {
                return Err(format!("action {} does not accept {name}", self.id));
            }
        }
        Ok(())
    }
}

fn bounded_delta(value: Option<i32>, action: &str, limit: i32) -> Result<i32, String> {
    let value = value.ok_or_else(|| format!("action {action} requires delta"))?;
    if value == 0 || value.abs() > limit {
        return Err(format!(
            "action {action} delta must be non-zero and within -{limit}..={limit}"
        ));
    }
    Ok(value)
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct KeyStroke {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
    pub key: String,
}

impl KeyStroke {
    pub fn parse(input: &str) -> Result<Self, String> {
        if input.len() > MAX_KEY_NOTATION_BYTES {
            return Err(format!(
                "key notation exceeds {MAX_KEY_NOTATION_BYTES} bytes"
            ));
        }
        if input.contains(',') {
            return Err("multi-stroke key sequences are not implemented".into());
        }
        let parts = input
            .split('+')
            .map(str::trim)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.is_empty() {
            return Err("key notation is empty".into());
        }

        let mut stroke = Self {
            ctrl: false,
            alt: false,
            shift: false,
            super_key: false,
            key: String::new(),
        };
        for part in parts {
            let modifier = match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => Some("Ctrl"),
                "alt" => Some("Alt"),
                "shift" => Some("Shift"),
                "super" | "meta" => Some("Super"),
                _ => None,
            };
            if let Some(modifier) = modifier {
                let slot = match modifier {
                    "Ctrl" => &mut stroke.ctrl,
                    "Alt" => &mut stroke.alt,
                    "Shift" => &mut stroke.shift,
                    "Super" => &mut stroke.super_key,
                    _ => unreachable!(),
                };
                if *slot {
                    return Err(format!("duplicate modifier {modifier}"));
                }
                *slot = true;
                continue;
            }

            if !stroke.key.is_empty() {
                return Err("key notation must contain exactly one non-modifier key".into());
            }
            stroke.key = canonical_key_name(part)?;
        }
        if stroke.key.is_empty() {
            return Err("key notation is missing a non-modifier key".into());
        }
        Ok(stroke)
    }

    pub fn from_wayland(key: &WaylandKey) -> Option<Self> {
        let (key, modifiers) = split_wayland_key(key);
        let key = wayland_key_name(key, modifiers)?;
        Some(Self {
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
            shift: modifiers.shift,
            super_key: modifiers.super_key,
            key,
        })
    }

    pub fn is_printable(&self) -> bool {
        self.key.len() == 1
            || matches!(
                self.key.as_str(),
                "Space"
                    | "Minus"
                    | "Equal"
                    | "BracketLeft"
                    | "BracketRight"
                    | "Semicolon"
                    | "Apostrophe"
                    | "Grave"
                    | "Backslash"
                    | "Comma"
                    | "Period"
                    | "Slash"
            )
    }
}

impl fmt::Display for KeyStroke {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.super_key {
            f.write_str("Super+")?;
        }
        f.write_str(&self.key)
    }
}

fn canonical_key_name(input: &str) -> Result<String, String> {
    let lower = input.to_ascii_lowercase();
    let named = match lower.as_str() {
        "enter" | "return" => Some("Enter"),
        "escape" | "esc" => Some("Escape"),
        "backspace" => Some("Backspace"),
        "tab" => Some("Tab"),
        "space" => Some("Space"),
        "up" => Some("Up"),
        "down" => Some("Down"),
        "left" => Some("Left"),
        "right" => Some("Right"),
        "minus" => Some("Minus"),
        "equal" => Some("Equal"),
        "bracketleft" => Some("BracketLeft"),
        "bracketright" => Some("BracketRight"),
        "semicolon" => Some("Semicolon"),
        "apostrophe" => Some("Apostrophe"),
        "grave" => Some("Grave"),
        "backslash" => Some("Backslash"),
        "comma" => Some("Comma"),
        "period" => Some("Period"),
        "slash" => Some("Slash"),
        "xf86audiomute" => Some("XF86AudioMute"),
        "xf86audiolowervolume" => Some("XF86AudioLowerVolume"),
        "xf86audioraisevolume" => Some("XF86AudioRaiseVolume"),
        "xf86audioprev" => Some("XF86AudioPrev"),
        "xf86audioplay" => Some("XF86AudioPlay"),
        "xf86audionext" => Some("XF86AudioNext"),
        "xf86audiostop" => Some("XF86AudioStop"),
        "xf86monbrightnessdown" => Some("XF86MonBrightnessDown"),
        "xf86monbrightnessup" => Some("XF86MonBrightnessUp"),
        _ => None,
    };
    if let Some(named) = named {
        return Ok(named.into());
    }

    if let Some(number) = lower
        .strip_prefix('f')
        .and_then(|value| value.parse::<u8>().ok())
    {
        if (1..=24).contains(&number) {
            return Ok(format!("F{number}"));
        }
    }

    let mut chars = input.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphabetic() {
            return Ok(ch.to_ascii_uppercase().to_string());
        }
        if ch.is_ascii_digit() {
            return Ok(ch.to_string());
        }
    }
    Err(format!("unknown key name {input:?}"))
}

fn wayland_key_name(key: &WaylandKey, modifiers: WaylandModifiers) -> Option<String> {
    match key {
        WaylandKey::Up => Some("Up".into()),
        WaylandKey::Down => Some("Down".into()),
        WaylandKey::Left => Some("Left".into()),
        WaylandKey::Right => Some("Right".into()),
        WaylandKey::Enter => Some("Enter".into()),
        WaylandKey::Escape => Some("Escape".into()),
        WaylandKey::Backspace => Some("Backspace".into()),
        WaylandKey::Tab => Some("Tab".into()),
        WaylandKey::Space => Some("Space".into()),
        WaylandKey::Function(number) if (1..=24).contains(number) => Some(format!("F{number}")),
        WaylandKey::Function(_) => None,
        WaylandKey::Media(media) => Some(
            match media {
                MediaKey::AudioMute => "XF86AudioMute",
                MediaKey::AudioLowerVolume => "XF86AudioLowerVolume",
                MediaKey::AudioRaiseVolume => "XF86AudioRaiseVolume",
                MediaKey::AudioPrevious => "XF86AudioPrev",
                MediaKey::AudioPlayPause => "XF86AudioPlay",
                MediaKey::AudioNext => "XF86AudioNext",
                MediaKey::AudioStop => "XF86AudioStop",
                MediaKey::MonBrightnessDown => "XF86MonBrightnessDown",
                MediaKey::MonBrightnessUp => "XF86MonBrightnessUp",
            }
            .into(),
        ),
        WaylandKey::Text(text) => printable_key_name(text, modifiers.shift),
        WaylandKey::Raw(_) => None,
        WaylandKey::Modified { .. } => None,
    }
}

fn split_wayland_key(key: &WaylandKey) -> (&WaylandKey, WaylandModifiers) {
    match key {
        WaylandKey::Modified { key, modifiers } => (key.as_ref(), *modifiers),
        other => (other, WaylandModifiers::default()),
    }
}

fn printable_key_name(text: &str, shifted: bool) -> Option<String> {
    let mut chars = text.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if ch.is_ascii_alphabetic() {
        return Some(ch.to_ascii_uppercase().to_string());
    }
    if ch.is_ascii_digit() {
        return Some(ch.to_string());
    }
    let base = if shifted {
        match ch {
            '!' => "1",
            '@' => "2",
            '#' => "3",
            '$' => "4",
            '%' => "5",
            '^' => "6",
            '&' => "7",
            '*' => "8",
            '(' => "9",
            ')' => "0",
            '_' => "Minus",
            '+' => "Equal",
            '{' => "BracketLeft",
            '}' => "BracketRight",
            ':' => "Semicolon",
            '"' => "Apostrophe",
            '~' => "Grave",
            '|' => "Backslash",
            '<' => "Comma",
            '>' => "Period",
            '?' => "Slash",
            _ => return None,
        }
    } else {
        match ch {
            '-' => "Minus",
            '=' => "Equal",
            '[' => "BracketLeft",
            ']' => "BracketRight",
            ';' => "Semicolon",
            '\'' => "Apostrophe",
            '`' => "Grave",
            '\\' => "Backslash",
            ',' => "Comma",
            '.' => "Period",
            '/' => "Slash",
            _ => return None,
        }
    };
    Some(base.into())
}

#[derive(Debug, Clone)]
struct BindingDefinition {
    id: String,
    stroke: KeyStroke,
    scope: BindingScope,
    action: ActionSpec,
    enabled: bool,
    source: BindingSource,
    replaces: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingSource {
    BuiltIn,
    UserConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BindingView {
    pub id: String,
    pub keys: String,
    pub scope: BindingScope,
    pub action: ActionSpec,
    pub source: BindingSource,
    pub enabled: bool,
    pub effective: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replaces: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeybindingDump {
    pub schema: &'static str,
    pub customization_enabled: bool,
    pub bindings: Vec<BindingView>,
}

#[derive(Debug, Clone)]
pub struct KeybindingRegistry {
    customization_enabled: bool,
    bindings: Vec<BindingDefinition>,
}

impl KeybindingRegistry {
    pub fn from_config(config: &KeybindingConfig) -> Result<Self, String> {
        if config.bindings.len() > MAX_USER_BINDINGS {
            return Err(format!(
                "keybindings.bindings exceeds {MAX_USER_BINDINGS} entries"
            ));
        }

        let mut by_id = builtin_bindings()
            .into_iter()
            .map(|binding| (binding.id.clone(), binding))
            .collect::<BTreeMap<_, _>>();

        let mut user_ids = BTreeSet::new();
        for entry in &config.bindings {
            validate_binding_id(&entry.id)?;
            if !user_ids.insert(entry.id.clone()) {
                return Err(format!(
                    "duplicate keybinding config entry id {:?}",
                    entry.id
                ));
            }
            validate_override(entry, by_id.get(&entry.id))?;
        }

        if config.enabled {
            for entry in &config.bindings {
                let previous = by_id.get(&entry.id).cloned();
                let definition = merge_override(entry, previous.as_ref())?;
                by_id.insert(entry.id.clone(), definition);
            }
        }

        let bindings = by_id.into_values().collect::<Vec<_>>();
        validate_collisions(&bindings)?;
        for binding in &bindings {
            binding.action.resolve()?;
            validate_scope_action(binding.scope, &binding.action)?;
        }

        Ok(Self {
            customization_enabled: config.enabled,
            bindings,
        })
    }

    pub fn dump(&self) -> KeybindingDump {
        let mut bindings = self
            .bindings
            .iter()
            .map(|binding| BindingView {
                id: binding.id.clone(),
                keys: binding.stroke.to_string(),
                scope: binding.scope,
                action: binding.action.clone(),
                source: binding.source,
                enabled: binding.enabled,
                effective: binding.enabled,
                replaces: binding.replaces.clone(),
            })
            .collect::<Vec<_>>();
        bindings.sort_by(|left, right| left.id.cmp(&right.id));
        KeybindingDump {
            schema: "nuraloumi-keybindings/v1",
            customization_enabled: self.customization_enabled,
            bindings,
        }
    }

    fn resolve<'a>(
        &'a self,
        stroke: &KeyStroke,
        shell: &ShellState,
    ) -> Option<&'a BindingDefinition> {
        for scope in scope_chain(shell) {
            if let Some(binding) = self.bindings.iter().find(|binding| {
                binding.enabled && binding.scope == scope && binding.stroke == *stroke
            }) {
                return Some(binding);
            }
        }
        None
    }
}

fn validate_binding_id(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > MAX_BINDING_ID_BYTES {
        return Err(format!(
            "keybinding id must be 1..={MAX_BINDING_ID_BYTES} bytes"
        ));
    }
    if !id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(format!("invalid keybinding id {id:?}"));
    }
    Ok(())
}

fn validate_override(
    entry: &BindingOverride,
    previous: Option<&BindingDefinition>,
) -> Result<(), String> {
    if let Some(keys) = &entry.keys {
        KeyStroke::parse(keys)?;
    }
    if let Some(action) = &entry.action {
        action.resolve()?;
    }
    if previous.is_none()
        && entry.enabled.unwrap_or(true)
        && (entry.keys.is_none() || entry.scope.is_none() || entry.action.is_none())
    {
        return Err(format!(
            "custom keybinding {:?} requires keys, scope, and action",
            entry.id
        ));
    }
    if previous.is_none() && entry.enabled == Some(false) {
        return Err(format!(
            "disabled custom keybinding {:?} has no built-in definition to disable",
            entry.id
        ));
    }
    Ok(())
}

fn merge_override(
    entry: &BindingOverride,
    previous: Option<&BindingDefinition>,
) -> Result<BindingDefinition, String> {
    let enabled = entry
        .enabled
        .unwrap_or_else(|| previous.map(|binding| binding.enabled).unwrap_or(true));
    let stroke = match (&entry.keys, previous) {
        (Some(keys), _) => KeyStroke::parse(keys)?,
        (None, Some(binding)) => binding.stroke.clone(),
        (None, None) => return Err(format!("keybinding {:?} requires keys", entry.id)),
    };
    let scope = entry
        .scope
        .or_else(|| previous.map(|binding| binding.scope))
        .ok_or_else(|| format!("keybinding {:?} requires scope", entry.id))?;
    let action = entry
        .action
        .clone()
        .or_else(|| previous.map(|binding| binding.action.clone()))
        .ok_or_else(|| format!("keybinding {:?} requires action", entry.id))?;
    action.resolve()?;
    validate_scope_action(scope, &action)?;
    Ok(BindingDefinition {
        id: entry.id.clone(),
        stroke,
        scope,
        action,
        enabled,
        source: BindingSource::UserConfig,
        replaces: previous.map(|binding| binding.id.clone()),
    })
}

fn validate_collisions(bindings: &[BindingDefinition]) -> Result<(), String> {
    let mut seen = BTreeMap::<(BindingScope, KeyStroke), String>::new();
    for binding in bindings.iter().filter(|binding| binding.enabled) {
        let key = (binding.scope, binding.stroke.clone());
        if let Some(existing) = seen.insert(key, binding.id.clone()) {
            return Err(format!(
                "keybinding collision in {:?}: {:?} and {:?} both use {}",
                binding.scope, existing, binding.id, binding.stroke
            ));
        }
    }
    Ok(())
}

fn validate_scope_action(scope: BindingScope, action: &ActionSpec) -> Result<(), String> {
    match action.id.as_str() {
        "overview.mode"
            if !matches!(
                scope,
                BindingScope::Launcher
                    | BindingScope::Applications
                    | BindingScope::Windows
                    | BindingScope::Tasks
                    | BindingScope::Desktops
            ) =>
        {
            Err("overview.mode bindings require a launcher-related scope".into())
        }
        "control.tab" if scope != BindingScope::ControlCenter => {
            Err("control.tab bindings require control-center scope".into())
        }
        _ => Ok(()),
    }
}

fn builtin_bindings() -> Vec<BindingDefinition> {
    [
        (
            "menu.up",
            "Up",
            BindingScope::Menu,
            ActionSpec::with_direction("up"),
        ),
        (
            "menu.down",
            "Down",
            BindingScope::Menu,
            ActionSpec::with_direction("down"),
        ),
        (
            "menu.left",
            "Left",
            BindingScope::Menu,
            ActionSpec::with_direction("left"),
        ),
        (
            "menu.right",
            "Right",
            BindingScope::Menu,
            ActionSpec::with_direction("right"),
        ),
        (
            "menu.activate",
            "Enter",
            BindingScope::Menu,
            ActionSpec::simple("menu.activate"),
        ),
        (
            "menu.escape",
            "Escape",
            BindingScope::Menu,
            ActionSpec::simple("menu.back-or-close"),
        ),
        (
            "menu.backspace",
            "Backspace",
            BindingScope::Menu,
            ActionSpec::simple("menu.backspace"),
        ),
        (
            "audio.volume-down",
            "XF86AudioLowerVolume",
            BindingScope::Global,
            ActionSpec::with_delta("audio.adjust", -5),
        ),
        (
            "audio.volume-up",
            "XF86AudioRaiseVolume",
            BindingScope::Global,
            ActionSpec::with_delta("audio.adjust", 5),
        ),
        (
            "audio.mute",
            "XF86AudioMute",
            BindingScope::Global,
            ActionSpec::simple("audio.toggle-mute"),
        ),
        (
            "display.brightness-down",
            "XF86MonBrightnessDown",
            BindingScope::Global,
            ActionSpec::with_delta("display.adjust", -10),
        ),
        (
            "display.brightness-up",
            "XF86MonBrightnessUp",
            BindingScope::Global,
            ActionSpec::with_delta("display.adjust", 10),
        ),
    ]
    .into_iter()
    .map(|(id, keys, scope, action)| BindingDefinition {
        id: id.into(),
        stroke: KeyStroke::parse(keys).expect("built-in key notation must be valid"),
        scope,
        action,
        enabled: true,
        source: BindingSource::BuiltIn,
        replaces: None,
    })
    .collect()
}

fn scope_chain(shell: &ShellState) -> Vec<BindingScope> {
    let mut scopes = Vec::with_capacity(4);
    match shell.menu.id.as_str() {
        "launcher" => {
            let detail = match shell.overview_mode {
                OverviewMode::All => None,
                OverviewMode::Windows => Some(BindingScope::Windows),
                OverviewMode::Apps => Some(BindingScope::Applications),
                OverviewMode::Tasks => Some(BindingScope::Tasks),
                OverviewMode::Desktops => Some(BindingScope::Desktops),
            };
            if let Some(detail) = detail {
                scopes.push(detail);
            }
            scopes.push(BindingScope::Launcher);
        }
        "control-center" => scopes.push(BindingScope::ControlCenter),
        "windows" => scopes.push(BindingScope::Windows),
        "tasks" => scopes.push(BindingScope::Tasks),
        _ => {}
    }
    scopes.push(BindingScope::Menu);
    scopes.push(BindingScope::Global);
    scopes
}

fn text_semantic(shell: &ShellState, key: &WaylandKey) -> Option<SemanticInput> {
    let (key, modifiers) = split_wayland_key(key);
    if !shell.search_focused || modifiers.ctrl || modifiers.alt || modifiers.super_key {
        return None;
    }
    match key {
        WaylandKey::Text(text) => Some(SemanticInput::Text(text.clone())),
        WaylandKey::Space => Some(SemanticInput::Text(" ".into())),
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindingDispatchAction {
    Semantic(SemanticInput),
    Menu(MenuAction),
}

pub fn dispatch_keybinding(
    registry: &KeybindingRegistry,
    shell: &mut ShellState,
    key: &WaylandKey,
) -> Option<ActionReport> {
    if let Some(input) = text_semantic(shell, key) {
        return Some(shell.apply_semantic(input));
    }
    let stroke = KeyStroke::from_wayland(key)?;
    let binding = registry.resolve(&stroke, shell)?;
    match binding
        .action
        .resolve()
        .expect("validated keybinding action must remain valid")
    {
        BindingDispatchAction::Semantic(input) => Some(shell.apply_semantic(input)),
        BindingDispatchAction::Menu(action) => Some(ActionReport::Dispatched {
            item_id: format!("keybinding:{}", binding.id),
            action,
            confirmed: false,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{build_launcher_menu, ShellState};

    fn shell() -> ShellState {
        ShellState::new(build_launcher_menu(), true).expect("shell")
    }

    #[test]
    fn canonical_notation_normalizes_aliases_and_modifier_order() {
        assert_eq!(
            KeyStroke::parse(" meta + control + a ")
                .expect("stroke")
                .to_string(),
            "Ctrl+Super+A"
        );
        assert_eq!(
            KeyStroke::parse("Shift+1").expect("stroke").to_string(),
            "Shift+1"
        );
        assert_eq!(
            KeyStroke::parse("xf86audioraisevolume")
                .expect("stroke")
                .to_string(),
            "XF86AudioRaiseVolume"
        );
        assert!(KeyStroke::parse("Super+K, W").is_err());
        assert!(KeyStroke::parse("Raw:30").is_err());
    }

    #[test]
    fn shifted_wayland_text_keeps_physical_key_identity() {
        let modifiers = WaylandModifiers {
            shift: true,
            ..WaylandModifiers::default()
        };
        assert_eq!(
            KeyStroke::from_wayland(&WaylandKey::Modified {
                key: Box::new(WaylandKey::Text("!".into())),
                modifiers,
            })
            .expect("stroke")
            .to_string(),
            "Shift+1"
        );
    }

    #[test]
    fn builtins_and_user_overrides_share_one_registry() {
        let config = KeybindingConfig {
            enabled: true,
            bindings: vec![BindingOverride {
                id: "menu.down".into(),
                enabled: None,
                keys: Some("Ctrl+J".into()),
                scope: None,
                action: None,
            }],
        };
        let registry = KeybindingRegistry::from_config(&config).expect("registry");
        let dump = registry.dump();
        let down = dump
            .bindings
            .iter()
            .find(|binding| binding.id == "menu.down")
            .expect("down");
        assert_eq!(down.keys, "Ctrl+J");
        assert_eq!(down.source, BindingSource::UserConfig);
        assert_eq!(down.replaces.as_deref(), Some("menu.down"));
    }

    #[test]
    fn collisions_are_rejected_without_order_dependent_winner() {
        let config = KeybindingConfig {
            enabled: true,
            bindings: vec![
                BindingOverride {
                    id: "user.one".into(),
                    enabled: None,
                    keys: Some("Ctrl+J".into()),
                    scope: Some(BindingScope::Menu),
                    action: Some(ActionSpec::simple("menu.activate")),
                },
                BindingOverride {
                    id: "user.two".into(),
                    enabled: None,
                    keys: Some("Ctrl+J".into()),
                    scope: Some(BindingScope::Menu),
                    action: Some(ActionSpec::simple("menu.back-or-close")),
                },
            ],
        };
        let error = KeybindingRegistry::from_config(&config).expect_err("collision");
        assert!(error.contains("user.one"));
        assert!(error.contains("user.two"));
    }

    #[test]
    fn duplicate_ids_and_unknown_actions_fail_validation() {
        let duplicate = KeybindingConfig {
            enabled: true,
            bindings: vec![
                BindingOverride {
                    id: "user.same".into(),
                    enabled: None,
                    keys: Some("Ctrl+J".into()),
                    scope: Some(BindingScope::Menu),
                    action: Some(ActionSpec::simple("menu.activate")),
                },
                BindingOverride {
                    id: "user.same".into(),
                    enabled: None,
                    keys: Some("Ctrl+K".into()),
                    scope: Some(BindingScope::Menu),
                    action: Some(ActionSpec::simple("menu.activate")),
                },
            ],
        };
        assert!(KeybindingRegistry::from_config(&duplicate)
            .expect_err("duplicate")
            .contains("duplicate keybinding config entry"));

        let unknown = KeybindingConfig {
            enabled: true,
            bindings: vec![BindingOverride {
                id: "user.unsafe".into(),
                enabled: None,
                keys: Some("Ctrl+X".into()),
                scope: Some(BindingScope::Global),
                action: Some(ActionSpec::simple("command.exec")),
            }],
        };
        assert!(KeybindingRegistry::from_config(&unknown)
            .expect_err("unknown action")
            .contains("unknown keybinding action id"));
    }

    #[test]
    fn specific_scope_wins_over_global_for_same_chord() {
        let config = KeybindingConfig {
            enabled: true,
            bindings: vec![
                BindingOverride {
                    id: "user.global".into(),
                    enabled: None,
                    keys: Some("Ctrl+K".into()),
                    scope: Some(BindingScope::Global),
                    action: Some(ActionSpec {
                        id: "menu.open".into(),
                        family: Some("audio".into()),
                        direction: None,
                        mode: None,
                        tab: None,
                        delta: None,
                    }),
                },
                BindingOverride {
                    id: "user.launcher".into(),
                    enabled: None,
                    keys: Some("Ctrl+K".into()),
                    scope: Some(BindingScope::Launcher),
                    action: Some(ActionSpec {
                        id: "menu.open".into(),
                        family: Some("power".into()),
                        direction: None,
                        mode: None,
                        tab: None,
                        delta: None,
                    }),
                },
            ],
        };
        let registry = KeybindingRegistry::from_config(&config).expect("registry");
        let mut shell = shell();
        let report = dispatch_keybinding(
            &registry,
            &mut shell,
            &WaylandKey::Modified {
                key: Box::new(WaylandKey::Text("k".into())),
                modifiers: WaylandModifiers {
                    ctrl: true,
                    ..WaylandModifiers::default()
                },
            },
        )
        .expect("report");
        assert!(matches!(
            report,
            ActionReport::Dispatched {
                action: MenuAction::Custom { ref kind, ref payload },
                ..
            } if kind == "menu.open" && payload == "power"
        ));
    }

    #[test]
    fn disabling_customization_preserves_builtin_baseline() {
        let config = KeybindingConfig {
            enabled: false,
            bindings: vec![BindingOverride {
                id: "menu.down".into(),
                enabled: None,
                keys: Some("Ctrl+J".into()),
                scope: None,
                action: None,
            }],
        };
        let registry = KeybindingRegistry::from_config(&config).expect("registry");
        let dump = registry.dump();
        assert!(!dump.customization_enabled);
        let down = dump
            .bindings
            .iter()
            .find(|binding| binding.id == "menu.down")
            .expect("down");
        assert_eq!(down.keys, "Down");
        assert_eq!(down.source, BindingSource::BuiltIn);
    }

    #[test]
    fn search_text_wins_over_unmodified_printable_binding() {
        let config = KeybindingConfig {
            enabled: true,
            bindings: vec![BindingOverride {
                id: "user.a".into(),
                enabled: None,
                keys: Some("A".into()),
                scope: Some(BindingScope::Launcher),
                action: Some(ActionSpec::simple("menu.activate")),
            }],
        };
        let registry = KeybindingRegistry::from_config(&config).expect("registry");
        let mut shell = shell();
        shell.focus_search(true);
        let report = dispatch_keybinding(&registry, &mut shell, &WaylandKey::Text("a".into()))
            .expect("report");
        assert!(matches!(report, ActionReport::SearchChanged { .. }));
        assert_eq!(shell.state.query, "a");
    }

    #[test]
    fn modified_printable_binding_resolves_while_search_is_focused() {
        let config = KeybindingConfig {
            enabled: true,
            bindings: vec![BindingOverride {
                id: "user.open-audio".into(),
                enabled: None,
                keys: Some("Ctrl+A".into()),
                scope: Some(BindingScope::Launcher),
                action: Some(ActionSpec {
                    id: "menu.open".into(),
                    family: Some("audio".into()),
                    direction: None,
                    mode: None,
                    tab: None,
                    delta: None,
                }),
            }],
        };
        let registry = KeybindingRegistry::from_config(&config).expect("registry");
        let mut shell = shell();
        shell.focus_search(true);
        let report = dispatch_keybinding(
            &registry,
            &mut shell,
            &WaylandKey::Modified {
                key: Box::new(WaylandKey::Text("a".into())),
                modifiers: WaylandModifiers {
                    ctrl: true,
                    ..WaylandModifiers::default()
                },
            },
        )
        .expect("report");
        assert!(matches!(
            report,
            ActionReport::Dispatched {
                action: MenuAction::Custom { ref kind, ref payload },
                ..
            } if kind == "menu.open" && payload == "audio"
        ));
        assert!(shell.state.query.is_empty());
    }

    #[test]
    fn disabling_builtin_is_explicit_and_introspectable() {
        let config = KeybindingConfig {
            enabled: true,
            bindings: vec![BindingOverride {
                id: "menu.escape".into(),
                enabled: Some(false),
                keys: None,
                scope: None,
                action: None,
            }],
        };
        let registry = KeybindingRegistry::from_config(&config).expect("registry");
        let view = registry
            .dump()
            .bindings
            .into_iter()
            .find(|binding| binding.id == "menu.escape")
            .expect("escape");
        assert!(!view.enabled);
        assert!(!view.effective);
        assert_eq!(view.source, BindingSource::UserConfig);
    }
}
