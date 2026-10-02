//! Deterministic validation for provider-supplied menu data.

use crate::model::{MenuAction, MenuItem, MenuItemKind, MenuModel};
use std::collections::HashSet;
use std::error::Error;
use std::fmt;

/// Bounded input limits for a menu tree.
///
/// Limits are deliberately conservative for the target tablet and may be
/// tightened by provider-specific code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ValidationLimits {
    pub max_id_chars: usize,
    pub max_text_chars: usize,
    pub max_action_payload_chars: usize,
    pub max_depth: usize,
    pub max_items: usize,
}

impl Default for ValidationLimits {
    fn default() -> Self {
        Self {
            max_id_chars: 128,
            max_text_chars: 512,
            max_action_payload_chars: 4096,
            max_depth: 8,
            max_items: 512,
        }
    }
}

impl ValidationLimits {
    /// Validate one complete menu tree.
    pub fn validate(self, model: &MenuModel) -> Result<(), ValidationError> {
        validate_id("menu.id", &model.id, self.max_id_chars)?;
        validate_required_text("menu.title", &model.title, self.max_text_chars)?;

        let mut seen = HashSet::new();
        let mut count = 0usize;
        validate_items(&model.items, 1, self, &mut seen, &mut count)
    }
}

/// Stable validation failure classes suitable for provider diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValidationError {
    EmptyId {
        field: String,
    },
    InvalidId {
        field: String,
        reason: &'static str,
    },
    DuplicateId {
        id: String,
    },
    TextTooLong {
        field: String,
        max_chars: usize,
        actual_chars: usize,
    },
    EmptyText {
        field: String,
    },
    TooDeep {
        item_id: String,
        max_depth: usize,
    },
    TooManyItems {
        max_items: usize,
    },
    InvalidStructure {
        item_id: String,
        reason: &'static str,
    },
    InvalidAction {
        item_id: String,
        reason: &'static str,
    },
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyId { field } => write!(f, "{field} must not be empty"),
            Self::InvalidId { field, reason } => write!(f, "{field} is invalid: {reason}"),
            Self::DuplicateId { id } => write!(f, "duplicate menu item id: {id}"),
            Self::TextTooLong {
                field,
                max_chars,
                actual_chars,
            } => write!(f, "{field} exceeds {max_chars} characters ({actual_chars})"),
            Self::EmptyText { field } => write!(f, "{field} must not be empty"),
            Self::TooDeep { item_id, max_depth } => {
                write!(f, "item {item_id} exceeds maximum depth {max_depth}")
            }
            Self::TooManyItems { max_items } => {
                write!(f, "menu exceeds maximum item count {max_items}")
            }
            Self::InvalidStructure { item_id, reason } => {
                write!(f, "item {item_id} has invalid structure: {reason}")
            }
            Self::InvalidAction { item_id, reason } => {
                write!(f, "item {item_id} has invalid action: {reason}")
            }
        }
    }
}

impl Error for ValidationError {}

fn validate_items(
    items: &[MenuItem],
    depth: usize,
    limits: ValidationLimits,
    seen: &mut HashSet<String>,
    count: &mut usize,
) -> Result<(), ValidationError> {
    for item in items {
        *count += 1;
        if *count > limits.max_items {
            return Err(ValidationError::TooManyItems {
                max_items: limits.max_items,
            });
        }

        if depth > limits.max_depth {
            return Err(ValidationError::TooDeep {
                item_id: item.id.clone(),
                max_depth: limits.max_depth,
            });
        }

        validate_id("item.id", &item.id, limits.max_id_chars)?;
        if !seen.insert(item.id.clone()) {
            return Err(ValidationError::DuplicateId {
                id: item.id.clone(),
            });
        }

        if item.kind != MenuItemKind::Separator {
            validate_required_text(
                &format!("item.{}.label", item.id),
                &item.label,
                limits.max_text_chars,
            )?;
        } else if !item.label.is_empty() || item.subtitle.is_some() {
            return Err(ValidationError::InvalidStructure {
                item_id: item.id.clone(),
                reason: "separator must not carry label or subtitle text",
            });
        }

        if let Some(subtitle) = &item.subtitle {
            validate_text(
                &format!("item.{}.subtitle", item.id),
                subtitle,
                limits.max_text_chars,
            )?;
        }

        validate_structure(item)?;

        if let Some(action) = &item.action {
            validate_action(action, &item.id, limits, false)?;
        }

        validate_items(&item.children, depth.saturating_add(1), limits, seen, count)?;
    }
    Ok(())
}

fn validate_structure(item: &MenuItem) -> Result<(), ValidationError> {
    let invalid = |reason| ValidationError::InvalidStructure {
        item_id: item.id.clone(),
        reason,
    };

    match item.kind {
        MenuItemKind::Action => {
            if item.enabled && item.action.is_none() {
                return Err(invalid("enabled action row requires a typed action"));
            }
            if !item.children.is_empty() {
                return Err(invalid("action row cannot contain children"));
            }
            if item.checked.is_some() {
                return Err(invalid("action row cannot carry checked state"));
            }
        }
        MenuItemKind::Submenu => {
            if item.action.is_some() {
                return Err(invalid(
                    "submenu navigation is represented by children, not an action",
                ));
            }
            if item.children.is_empty() {
                return Err(invalid("submenu requires at least one child"));
            }
            if item.checked.is_some() {
                return Err(invalid("submenu cannot carry checked state"));
            }
        }
        MenuItemKind::Checkable => {
            if !item.children.is_empty() {
                return Err(invalid("checkable row cannot contain children"));
            }
            if item.enabled && !matches!(item.action, Some(MenuAction::Toggle { .. })) {
                return Err(invalid("enabled checkable row requires a toggle action"));
            }
            if item.action.is_some() && !matches!(item.action, Some(MenuAction::Toggle { .. })) {
                return Err(invalid("checkable row may only carry a toggle action"));
            }
        }
        MenuItemKind::Section | MenuItemKind::Status => {
            if item.enabled {
                return Err(invalid("informational row must be disabled"));
            }
            if item.action.is_some() || !item.children.is_empty() || item.checked.is_some() {
                return Err(invalid(
                    "informational row cannot carry action, children, or checked state",
                ));
            }
        }
        MenuItemKind::Separator => {
            if item.enabled {
                return Err(invalid("separator must be disabled"));
            }
            if item.action.is_some() || !item.children.is_empty() || item.checked.is_some() {
                return Err(invalid(
                    "separator cannot carry action, children, or checked state",
                ));
            }
        }
    }

    Ok(())
}

fn validate_action(
    action: &MenuAction,
    owner_id: &str,
    limits: ValidationLimits,
    nested_confirmation: bool,
) -> Result<(), ValidationError> {
    match action {
        MenuAction::Activate { id } | MenuAction::Toggle { id } | MenuAction::Adjust { id, .. } => {
            validate_id("action.id", id, limits.max_id_chars)?;
        }
        MenuAction::Navigate { target } => {
            validate_id("action.target", target, limits.max_id_chars)?;
        }
        MenuAction::Confirm {
            confirmation,
            action,
        } => {
            if nested_confirmation {
                return Err(ValidationError::InvalidAction {
                    item_id: owner_id.to_string(),
                    reason: "confirmation actions cannot be nested",
                });
            }
            validate_required_text(
                "confirmation.title",
                &confirmation.title,
                limits.max_text_chars,
            )?;
            if let Some(body) = &confirmation.body {
                validate_text("confirmation.body", body, limits.max_text_chars)?;
            }
            validate_action(action, owner_id, limits, true)?;
        }
        MenuAction::Close => {}
        MenuAction::Custom { kind, payload } => {
            validate_required_text("custom.kind", kind, limits.max_id_chars)?;
            validate_text("custom.payload", payload, limits.max_action_payload_chars)?;
        }
    }
    Ok(())
}

fn validate_id(field: &str, value: &str, max_chars: usize) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::EmptyId {
            field: field.to_string(),
        });
    }
    if value.trim() != value {
        return Err(ValidationError::InvalidId {
            field: field.to_string(),
            reason: "leading or trailing whitespace is not allowed",
        });
    }
    if value.chars().any(char::is_control) {
        return Err(ValidationError::InvalidId {
            field: field.to_string(),
            reason: "control characters are not allowed",
        });
    }
    validate_text(field, value, max_chars)
}

fn validate_required_text(
    field: &str,
    value: &str,
    max_chars: usize,
) -> Result<(), ValidationError> {
    if value.is_empty() {
        return Err(ValidationError::EmptyText {
            field: field.to_string(),
        });
    }
    validate_text(field, value, max_chars)
}

fn validate_text(field: &str, value: &str, max_chars: usize) -> Result<(), ValidationError> {
    let actual_chars = value.chars().count();
    if actual_chars > max_chars {
        return Err(ValidationError::TextTooLong {
            field: field.to_string(),
            max_chars,
            actual_chars,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConfirmationKind, MenuAction, MenuItem, MenuModel};

    fn valid_model() -> MenuModel {
        MenuModel::new(
            "system",
            "System",
            vec![
                MenuItem::section("section:radio", "Radios"),
                MenuItem::checkable("wifi", "Wi-Fi", "wifi:set", true),
                MenuItem::submenu(
                    "power",
                    "Power",
                    vec![MenuItem::action(
                        "power-off",
                        "Power off",
                        MenuAction::destructive(
                            "Power off?",
                            Some("Unsaved work may be lost.".into()),
                            MenuAction::Custom {
                                kind: "system.power_off".into(),
                                payload: String::new(),
                            },
                        ),
                    )],
                ),
            ],
        )
    }

    #[test]
    fn valid_tree_passes() {
        ValidationLimits::default()
            .validate(&valid_model())
            .unwrap();
    }

    #[test]
    fn duplicate_ids_are_rejected_globally() {
        let model = MenuModel::new(
            "demo",
            "Demo",
            vec![
                MenuItem::action("same", "One", MenuAction::Activate { id: "one".into() }),
                MenuItem::submenu(
                    "nested",
                    "Nested",
                    vec![MenuItem::action(
                        "same",
                        "Two",
                        MenuAction::Activate { id: "two".into() },
                    )],
                ),
            ],
        );
        assert!(matches!(
            ValidationLimits::default().validate(&model),
            Err(ValidationError::DuplicateId { id }) if id == "same"
        ));
    }

    #[test]
    fn invalid_structural_combinations_fail_closed() {
        let mut bad = MenuItem::section("section:bad", "Bad");
        bad.enabled = true;
        let model = MenuModel::new("demo", "Demo", vec![bad]);
        assert!(matches!(
            ValidationLimits::default().validate(&model),
            Err(ValidationError::InvalidStructure { .. })
        ));

        let mut checkable = MenuItem::checkable("toggle", "Toggle", "toggle", false);
        checkable.action = Some(MenuAction::Close);
        let model = MenuModel::new("demo", "Demo", vec![checkable]);
        assert!(matches!(
            ValidationLimits::default().validate(&model),
            Err(ValidationError::InvalidStructure { .. })
        ));
    }

    #[test]
    fn depth_item_and_text_limits_are_deterministic() {
        let deep = MenuItem::submenu(
            "one",
            "One",
            vec![MenuItem::submenu(
                "two",
                "Two",
                vec![MenuItem::action(
                    "three",
                    "Three",
                    MenuAction::Activate { id: "three".into() },
                )],
            )],
        );
        let model = MenuModel::new("demo", "Demo", vec![deep]);
        let limits = ValidationLimits {
            max_depth: 2,
            ..ValidationLimits::default()
        };
        assert!(matches!(
            limits.validate(&model),
            Err(ValidationError::TooDeep { .. })
        ));

        let model = MenuModel::new(
            "demo",
            "Demo",
            vec![
                MenuItem::status("a", "A"),
                MenuItem::status("b", "B"),
                MenuItem::status("c", "C"),
            ],
        );
        let limits = ValidationLimits {
            max_items: 2,
            ..ValidationLimits::default()
        };
        assert_eq!(
            limits.validate(&model),
            Err(ValidationError::TooManyItems { max_items: 2 })
        );

        let model = MenuModel::new("demo", "Demo", vec![MenuItem::status("status", "abcdef")]);
        let limits = ValidationLimits {
            max_text_chars: 5,
            ..ValidationLimits::default()
        };
        assert!(matches!(
            limits.validate(&model),
            Err(ValidationError::TextTooLong { .. })
        ));
    }

    #[test]
    fn disabled_interactive_rows_may_omit_actions_and_state() {
        let mut action = MenuItem::action(
            "offline-action",
            "Unavailable action",
            MenuAction::Activate { id: "noop".into() },
        )
        .disabled();
        action.action = None;

        let mut checkable = MenuItem::checkable("offline-toggle", "Unavailable toggle", "toggle", false)
            .disabled();
        checkable.action = None;
        checkable.checked = None;

        let model = MenuModel::new("offline", "Offline", vec![action, checkable]);
        ValidationLimits::default().validate(&model).unwrap();
    }

    #[test]
    fn nested_confirmations_are_rejected() {
        let inner = MenuAction::Confirm {
            confirmation: crate::model::Confirmation {
                title: "Inner".into(),
                body: None,
                kind: ConfirmationKind::Standard,
            },
            action: Box::new(MenuAction::Close),
        };
        let outer = MenuAction::destructive("Outer", None, inner);
        let model = MenuModel::new("demo", "Demo", vec![MenuItem::action("run", "Run", outer)]);
        assert!(matches!(
            ValidationLimits::default().validate(&model),
            Err(ValidationError::InvalidAction { .. })
        ));
    }
}
