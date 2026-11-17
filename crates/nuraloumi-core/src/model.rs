//! Stable renderer-neutral menu data.

use serde::{Deserialize, Serialize};

/// Complete menu snapshot published by a provider or assembled by the shell.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuModel {
    /// Stable menu identity. It must not depend on row position or presentation.
    pub id: String,
    /// Human-readable surface title.
    pub title: String,
    /// Ordered semantic rows.
    #[serde(default)]
    pub items: Vec<MenuItem>,
}

impl MenuModel {
    /// Construct a menu snapshot.
    pub fn new(id: impl Into<String>, title: impl Into<String>, items: Vec<MenuItem>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            items,
        }
    }

    /// Find an item anywhere in the tree by its globally unique semantic ID.
    pub fn item_by_id(&self, id: &str) -> Option<&MenuItem> {
        fn find<'a>(items: &'a [MenuItem], id: &str) -> Option<&'a MenuItem> {
            for item in items {
                if item.id == id {
                    return Some(item);
                }
                if let Some(found) = find(&item.children, id) {
                    return Some(found);
                }
            }
            None
        }

        find(&self.items, id)
    }

    /// Validate this complete menu tree using the production core limits.
    pub fn validate(&self) -> Result<(), crate::ValidationError> {
        crate::validate_menu(self)
    }
}

/// Semantic row kind. Renderers may style these differently, but must not alter
/// their interaction semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuItemKind {
    Action,
    Submenu,
    Checkable,
    Section,
    Status,
    Separator,
}

impl MenuItemKind {
    /// Whether rows of this kind can participate in keyboard/touch activation.
    pub const fn is_interactive(self) -> bool {
        matches!(self, Self::Action | Self::Submenu | Self::Checkable)
    }
}

fn default_true() -> bool {
    true
}

/// One ordered semantic menu row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MenuItem {
    /// Stable identity, unique across the complete menu tree.
    pub id: String,
    /// Primary row label.
    pub label: String,
    /// Optional secondary text.
    #[serde(default)]
    pub subtitle: Option<String>,
    /// Semantic row kind.
    pub kind: MenuItemKind,
    /// Whether interaction is currently permitted.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Whether the row is visible to navigation and rendering.
    #[serde(default = "default_true")]
    pub visible: bool,
    /// Typed action returned to the shell on activation.
    #[serde(default)]
    pub action: Option<MenuAction>,
    /// Nested rows for submenu items.
    #[serde(default)]
    pub children: Vec<MenuItem>,
    /// Check state for checkable rows. None represents an unknown current state.
    #[serde(default)]
    pub checked: Option<bool>,
}

impl MenuItem {
    /// Construct a normal action row.
    pub fn action(id: impl Into<String>, label: impl Into<String>, action: MenuAction) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            kind: MenuItemKind::Action,
            enabled: true,
            visible: true,
            action: Some(action),
            children: Vec::new(),
            checked: None,
        }
    }

    /// Construct a submenu row.
    pub fn submenu(
        id: impl Into<String>,
        label: impl Into<String>,
        children: Vec<MenuItem>,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            kind: MenuItemKind::Submenu,
            enabled: true,
            visible: true,
            action: None,
            children,
            checked: None,
        }
    }

    /// Construct a checkable row backed by a typed toggle action.
    pub fn checkable(
        id: impl Into<String>,
        label: impl Into<String>,
        toggle_id: impl Into<String>,
        checked: bool,
    ) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            kind: MenuItemKind::Checkable,
            enabled: true,
            visible: true,
            action: Some(MenuAction::Toggle {
                id: toggle_id.into(),
            }),
            children: Vec::new(),
            checked: Some(checked),
        }
    }

    /// Construct a non-interactive section heading.
    pub fn section(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::informational(id, label, MenuItemKind::Section)
    }

    /// Construct a non-interactive status row.
    pub fn status(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self::informational(id, label, MenuItemKind::Status)
    }

    /// Construct a non-interactive separator.
    pub fn separator(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            label: String::new(),
            subtitle: None,
            kind: MenuItemKind::Separator,
            enabled: false,
            visible: true,
            action: None,
            children: Vec::new(),
            checked: None,
        }
    }

    fn informational(id: impl Into<String>, label: impl Into<String>, kind: MenuItemKind) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            subtitle: None,
            kind,
            enabled: false,
            visible: true,
            action: None,
            children: Vec::new(),
            checked: None,
        }
    }

    /// Attach a subtitle.
    pub fn with_subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Mark an interactive row disabled.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Hide a row from rendering, navigation, filtering, and quick-select.
    pub fn hidden(mut self) -> Self {
        self.visible = false;
        self
    }

    /// True only for visible, enabled rows whose semantic kind is interactive.
    pub const fn is_actionable(&self) -> bool {
        self.visible && self.enabled && self.kind.is_interactive()
    }
}

/// Typed operation requested by menu activation.
///
/// Actions are data only. The core never executes commands, talks to services,
/// or mutates the desktop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MenuAction {
    Activate {
        id: String,
    },
    Toggle {
        id: String,
    },
    Adjust {
        id: String,
        delta: i32,
    },
    /// Absolute bounded value assignment; providers remain responsible for applying it.
    SetValue {
        id: String,
        value: i32,
    },
    Navigate {
        target: String,
    },
    Confirm {
        confirmation: Confirmation,
        action: Box<MenuAction>,
    },
    Close,
    Custom {
        kind: String,
        payload: String,
    },
}

impl MenuAction {
    /// Wrap an action in a normal confirmation step.
    pub fn confirmed(title: impl Into<String>, body: Option<String>, action: MenuAction) -> Self {
        Self::Confirm {
            confirmation: Confirmation {
                title: title.into(),
                body,
                kind: ConfirmationKind::Standard,
            },
            action: Box::new(action),
        }
    }

    /// Wrap an action in an explicit destructive confirmation step.
    pub fn destructive(title: impl Into<String>, body: Option<String>, action: MenuAction) -> Self {
        Self::Confirm {
            confirmation: Confirmation {
                title: title.into(),
                body,
                kind: ConfirmationKind::Destructive,
            },
            action: Box::new(action),
        }
    }
}

/// Typed, presentation-independent compact controls. This is additive to
/// legacy MenuItem JSON: older menu snapshots retain their exact encoding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompactControl {
    /// A 48-logical-pixel minimum hit target; icon is a semantic ASCII fallback.
    Button {
        id: String,
        label: String,
        icon: String,
        enabled: bool,
        action: MenuAction,
    },
    /// A group is horizontal, with each button independently addressable.
    Toolbar {
        id: String,
        buttons: Vec<CompactButton>,
    },
    /// Integer bounds are inclusive. Non-divisible maxima are reachable.
    Slider {
        id: String,
        label: String,
        value_id: String,
        min: i32,
        max: i32,
        step: i32,
        value: i32,
        enabled: bool,
    },
    /// The currently selected tab has no action; other tabs navigate by typed action.
    Tabs {
        id: String,
        tabs: Vec<CompactTab>,
    },
    Breadcrumb {
        id: String,
        parts: Vec<String>,
    },
    Status {
        id: String,
        fields: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactButton {
    pub id: String,
    pub label: String,
    pub icon: String,
    pub enabled: bool,
    pub action: MenuAction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactTab {
    pub id: String,
    pub label: String,
    pub selected: bool,
    pub action: MenuAction,
}

/// Pure keyboard adjustments for a focused compact slider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompactSliderKey {
    Decrease,
    Increase,
    Minimum,
    Maximum,
}

/// Prevent duplicate IDs across the entire control stack.
pub fn validate_compact_controls(controls: &[CompactControl]) -> Result<(), &'static str> {
    let mut seen = std::collections::HashSet::new();
    for control in controls {
        control.validate()?;
        let id = match control {
            CompactControl::Button { id, .. }
            | CompactControl::Toolbar { id, .. }
            | CompactControl::Slider { id, .. }
            | CompactControl::Tabs { id, .. }
            | CompactControl::Breadcrumb { id, .. }
            | CompactControl::Status { id, .. } => id,
        };
        if !seen.insert(id.as_str()) {
            return Err("duplicate control id");
        }
        match control {
            CompactControl::Toolbar { buttons, .. } => {
                for b in buttons {
                    if !seen.insert(b.id.as_str()) {
                        return Err("duplicate button id");
                    }
                }
            }
            CompactControl::Tabs { tabs, .. } => {
                for t in tabs {
                    if !seen.insert(t.id.as_str()) {
                        return Err("duplicate tab id");
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

impl CompactControl {
    /// Clamp then quantize toward the nearest permitted step, preserving max.
    pub fn quantize(min: i32, max: i32, step: i32, value: i32) -> Option<i32> {
        if min >= max || step <= 0 {
            return None;
        }
        if value >= max {
            return Some(max);
        }
        if value <= min {
            return Some(min);
        }
        let delta = i64::from(value) - i64::from(min);
        let step = i64::from(step);
        let snapped = i64::from(min) + ((delta + step / 2) / step) * step;
        Some(snapped.clamp(i64::from(min), i64::from(max)) as i32)
    }

    /// Value emitted by touch/drag/keyboard positioning, never an implicit delta.
    pub fn slider_action(&self, value: i32) -> Option<MenuAction> {
        let Self::Slider {
            value_id,
            min,
            max,
            step,
            enabled: true,
            ..
        } = self
        else {
            return None;
        };
        Some(MenuAction::SetValue {
            id: value_id.clone(),
            value: Self::quantize(*min, *max, *step, value)?,
        })
    }

    pub fn slider_key_action(&self, key: CompactSliderKey) -> Option<MenuAction> {
        let Self::Slider {
            min,
            max,
            step,
            value,
            ..
        } = self
        else {
            return None;
        };
        let candidate = match key {
            CompactSliderKey::Decrease => value.saturating_sub(*step),
            CompactSliderKey::Increase => value.saturating_add(*step),
            CompactSliderKey::Minimum => *min,
            CompactSliderKey::Maximum => *max,
        };
        self.slider_action(candidate)
    }

    pub fn validate(&self) -> Result<(), &'static str> {
        match self {
            Self::Button {
                id, label, icon, ..
            } => {
                if id.is_empty() || label.is_empty() || icon.chars().count() > 4 {
                    return Err("invalid button");
                }
            }
            Self::Toolbar { id, buttons } => {
                if id.is_empty() || buttons.is_empty() || buttons.len() > 8 {
                    return Err("invalid toolbar");
                }
                let mut seen = std::collections::HashSet::new();
                for b in buttons {
                    if b.id.is_empty()
                        || b.label.is_empty()
                        || b.icon.chars().count() > 4
                        || !seen.insert(&b.id)
                    {
                        return Err("invalid toolbar button");
                    }
                }
            }
            Self::Slider {
                id,
                label,
                value_id,
                min,
                max,
                step,
                value,
                ..
            } => {
                if id.is_empty()
                    || label.is_empty()
                    || value_id.is_empty()
                    || Self::quantize(*min, *max, *step, *value).is_none()
                {
                    return Err("invalid slider");
                }
            }
            Self::Tabs { id, tabs } => {
                if id.is_empty()
                    || tabs.is_empty()
                    || tabs.len() > 8
                    || tabs.iter().filter(|t| t.selected).count() != 1
                {
                    return Err("invalid tabs");
                }
                let mut seen = std::collections::HashSet::new();
                for t in tabs {
                    if t.id.is_empty() || t.label.is_empty() || !seen.insert(&t.id) {
                        return Err("invalid tab");
                    }
                }
            }
            Self::Breadcrumb { id, parts } | Self::Status { id, fields: parts } => {
                if id.is_empty() || parts.is_empty() || parts.len() > 8 {
                    return Err("invalid status section");
                }
            }
        }
        Ok(())
    }
}

/// Confirmation presentation and safety semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Confirmation {
    pub title: String,
    pub body: Option<String>,
    pub kind: ConfirmationKind,
}

/// Explicit confirmation severity. Destructive actions must use Destructive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConfirmationKind {
    Standard,
    Destructive,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_constructors_encode_semantics() {
        let action = MenuItem::action(
            "terminal",
            "Terminal",
            MenuAction::Activate {
                id: "app:terminal".into(),
            },
        );
        assert!(action.is_actionable());
        assert_eq!(action.checked, None);

        let checkable = MenuItem::checkable("wifi", "Wi-Fi", "wifi:set", true);
        assert!(checkable.is_actionable());
        assert_eq!(checkable.checked, Some(true));

        let section = MenuItem::section("section:apps", "Apps");
        assert!(!section.is_actionable());

        let separator = MenuItem::separator("sep:one");
        assert!(!separator.is_actionable());
        assert!(separator.label.is_empty());
    }

    #[test]
    fn recursive_lookup_uses_semantic_identity() {
        let child = MenuItem::action(
            "child",
            "Child",
            MenuAction::Activate { id: "child".into() },
        );
        let model = MenuModel::new(
            "demo",
            "Demo",
            vec![MenuItem::submenu("parent", "Parent", vec![child])],
        );
        assert_eq!(
            model.item_by_id("child").map(|item| item.label.as_str()),
            Some("Child")
        );
    }
}

#[cfg(test)]
mod compact_contract_tests {
    use super::*;
    #[test]
    fn bounded_steps_extremes_and_overflow() {
        assert_eq!(CompactControl::quantize(0, 94, 10, 94), Some(94));
        assert_eq!(CompactControl::quantize(0, 94, 10, -50), Some(0));
        assert_eq!(CompactControl::quantize(0, 94, 10, 43), Some(40));
        assert_eq!(
            CompactControl::quantize(i32::MIN, i32::MAX, 100, i32::MAX),
            Some(i32::MAX)
        );
        assert_eq!(CompactControl::quantize(0, 100, 0, 50), None);
    }
    #[test]
    fn typed_slider_json_round_trip_legacy_remains_valid() {
        let slider = CompactControl::Slider {
            id: "volume".into(),
            label: "Volume".into(),
            value_id: "audio.volume".into(),
            min: 0,
            max: 100,
            step: 5,
            value: 48,
            enabled: true,
        };
        slider.validate().unwrap();
        let back: CompactControl =
            serde_json::from_str(&serde_json::to_string(&slider).unwrap()).unwrap();
        assert_eq!(slider, back);
        assert_eq!(
            slider.slider_action(78),
            Some(MenuAction::SetValue {
                id: "audio.volume".into(),
                value: 80
            })
        );
        let legacy = MenuModel::new(
            "old",
            "Old",
            vec![MenuItem::action(
                "x",
                "X",
                MenuAction::Activate { id: "x".into() },
            )],
        );
        assert_eq!(
            serde_json::from_str::<MenuModel>(&serde_json::to_string(&legacy).unwrap()).unwrap(),
            legacy
        );
    }
}

#[cfg(test)]
mod compact_validation_tests {
    use super::*;
    #[test]
    fn unique_ids_and_absolute_keyboard_slider_actions() {
        let mk = || CompactControl::Button {
            id: "same".into(),
            label: "A".into(),
            icon: "+".into(),
            enabled: true,
            action: MenuAction::Close,
        };
        assert_eq!(
            validate_compact_controls(&[mk(), mk()]),
            Err("duplicate control id")
        );
        let slider = CompactControl::Slider {
            id: "volume".into(),
            label: "Volume".into(),
            value_id: "audio.set".into(),
            min: 0,
            max: 94,
            step: 10,
            value: 90,
            enabled: true,
        };
        assert_eq!(
            slider.slider_key_action(CompactSliderKey::Increase),
            Some(MenuAction::SetValue {
                id: "audio.set".into(),
                value: 94
            })
        );
        assert_eq!(
            slider.slider_key_action(CompactSliderKey::Minimum),
            Some(MenuAction::SetValue {
                id: "audio.set".into(),
                value: 0
            })
        );
    }
}
