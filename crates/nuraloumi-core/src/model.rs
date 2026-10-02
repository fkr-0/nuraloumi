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
