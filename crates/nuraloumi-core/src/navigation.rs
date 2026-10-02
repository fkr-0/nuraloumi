//! Deterministic identity-based menu navigation state machine.

use crate::model::{Confirmation, MenuAction, MenuItem, MenuItemKind, MenuModel};
use crate::search::filter_visible_rows;
use serde::{Deserialize, Serialize};

/// Maximum query length accepted by the semantic state machine.
pub const MAX_QUERY_CHARS: usize = 256;

/// Navigation state owned by the shell.
///
/// selected_id is a semantic row identity, never a row index. path contains the
/// submenu item IDs from root to the currently open level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct MenuState {
    pub selected_id: Option<String>,
    pub path: Vec<String>,
    pub query: String,
}

impl MenuState {
    /// Create root state and select the first actionable row.
    pub fn new(model: &MenuModel) -> Self {
        let mut state = Self::default();
        state.normalize(model);
        state
    }

    /// Apply one semantic input and return an explicit outcome.
    ///
    /// No action is executed by this function.
    pub fn handle(&mut self, model: &MenuModel, input: SemanticInput) -> NavigationOutcome {
        self.normalize_path_and_query(model);
        if !matches!(input, SemanticInput::Activate) {
            self.normalize_selection(model);
        }

        match input {
            SemanticInput::Up => self.move_selection(model, -1),
            SemanticInput::Down => self.move_selection(model, 1),
            SemanticInput::Right => self.enter_selected_submenu(model),
            SemanticInput::Left => self.leave_submenu(model),
            SemanticInput::Activate => self.activate_selected(model),
            SemanticInput::Back | SemanticInput::Escape => {
                if self.path.is_empty() {
                    NavigationOutcome::CloseRequested
                } else {
                    self.leave_submenu(model)
                }
            }
            SemanticInput::Text(text) => {
                append_bounded(&mut self.query, &text, MAX_QUERY_CHARS);
                self.normalize_selection(model);
                NavigationOutcome::QueryChanged {
                    query: self.query.clone(),
                    selected_id: self.selected_id.clone(),
                }
            }
            SemanticInput::Backspace => {
                self.query.pop();
                self.normalize_selection(model);
                NavigationOutcome::QueryChanged {
                    query: self.query.clone(),
                    selected_id: self.selected_id.clone(),
                }
            }
        }
    }

    /// Repair a stale path/selection against a changed model.
    ///
    /// Invalid submenu suffixes are removed, while an existing semantic
    /// selection survives whenever it is still actionable and visible under the
    /// current filter.
    pub fn normalize(&mut self, model: &MenuModel) {
        self.normalize_path_and_query(model);
        self.normalize_selection(model);
    }

    fn normalize_path_and_query(&mut self, model: &MenuModel) {
        while items_at_path(model, &self.path).is_none() {
            if self.path.pop().is_none() {
                break;
            }
        }
        if self.query.chars().count() > MAX_QUERY_CHARS {
            self.query = self.query.chars().take(MAX_QUERY_CHARS).collect();
        }
    }

    fn normalize_selection(&mut self, model: &MenuModel) {
        let ids = self.actionable_ids(model);
        if self
            .selected_id
            .as_ref()
            .is_some_and(|selected| ids.iter().any(|id| id == selected))
        {
            return;
        }
        self.selected_id = ids.into_iter().next();
    }

    fn move_selection(&mut self, model: &MenuModel, direction: isize) -> NavigationOutcome {
        let ids = self.actionable_ids(model);
        if ids.is_empty() {
            self.selected_id = None;
            return NavigationOutcome::SelectionChanged { selected_id: None };
        }

        let current = self
            .selected_id
            .as_ref()
            .and_then(|selected| ids.iter().position(|id| id == selected))
            .unwrap_or(0);

        let next = if direction < 0 {
            if current == 0 {
                ids.len() - 1
            } else {
                current - 1
            }
        } else {
            (current + 1) % ids.len()
        };

        self.selected_id = Some(ids[next].clone());
        NavigationOutcome::SelectionChanged {
            selected_id: self.selected_id.clone(),
        }
    }

    fn enter_selected_submenu(&mut self, model: &MenuModel) -> NavigationOutcome {
        let Some(selected_id) = self.selected_id.clone() else {
            return NavigationOutcome::Noop;
        };
        let Some(item) = self.selected_item(model) else {
            return NavigationOutcome::Noop;
        };
        if item.kind != MenuItemKind::Submenu || !item.is_actionable() || item.children.is_empty() {
            return NavigationOutcome::Noop;
        }

        self.path.push(selected_id.clone());
        self.query.clear();
        self.selected_id = first_actionable_id(&item.children, "");
        NavigationOutcome::SubmenuEntered {
            item_id: selected_id,
            selected_id: self.selected_id.clone(),
        }
    }

    fn leave_submenu(&mut self, model: &MenuModel) -> NavigationOutcome {
        let Some(item_id) = self.path.pop() else {
            return NavigationOutcome::Noop;
        };
        self.query.clear();
        self.selected_id = Some(item_id.clone());
        self.normalize_selection(model);
        NavigationOutcome::SubmenuExited {
            item_id,
            selected_id: self.selected_id.clone(),
        }
    }

    fn activate_selected(&mut self, model: &MenuModel) -> NavigationOutcome {
        let Some(item) = self.selected_item(model) else {
            return NavigationOutcome::Noop;
        };
        if !item.is_actionable() {
            return NavigationOutcome::Noop;
        }

        if item.kind == MenuItemKind::Submenu {
            return self.enter_selected_submenu(model);
        }

        let Some(action) = item.action.clone() else {
            return NavigationOutcome::Noop;
        };
        let item_id = item.id.clone();

        match action {
            MenuAction::Confirm {
                confirmation,
                action,
            } => NavigationOutcome::ConfirmationRequested {
                item_id,
                confirmation,
                action: *action,
            },
            action => NavigationOutcome::ActionRequested { item_id, action },
        }
    }

    /// Return the current visible rows after resolving submenu path and query.
    ///
    /// The returned order is the canonical semantic order used by navigation,
    /// rendering adapters, hit testing, and quick-select.
    pub fn visible_items<'a>(&self, model: &'a MenuModel) -> Vec<&'a MenuItem> {
        let Some(items) = items_at_path(model, &self.path) else {
            return Vec::new();
        };
        filter_visible_rows(items, &self.query)
    }

    /// Resolve the currently selected actionable row by semantic identity.
    pub fn selected_item<'a>(&self, model: &'a MenuModel) -> Option<&'a MenuItem> {
        let selected = self.selected_id.as_deref()?;
        self.visible_items(model)
            .into_iter()
            .find(|item| item.id == selected && item.is_actionable())
    }

    fn actionable_ids(&self, model: &MenuModel) -> Vec<String> {
        self.visible_items(model)
            .into_iter()
            .filter(|item| item.is_actionable())
            .map(|item| item.id.clone())
            .collect()
    }
}

/// Normalized semantic input emitted by keyboard, pointer, touch, or an OSK.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SemanticInput {
    Up,
    Down,
    Left,
    Right,
    Activate,
    Back,
    Escape,
    Text(String),
    Backspace,
}

/// Explicit state-machine result for the shell to interpret.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavigationOutcome {
    Noop,
    SelectionChanged {
        selected_id: Option<String>,
    },
    SubmenuEntered {
        item_id: String,
        selected_id: Option<String>,
    },
    SubmenuExited {
        item_id: String,
        selected_id: Option<String>,
    },
    QueryChanged {
        query: String,
        selected_id: Option<String>,
    },
    ActionRequested {
        item_id: String,
        action: MenuAction,
    },
    ConfirmationRequested {
        item_id: String,
        confirmation: Confirmation,
        action: MenuAction,
    },
    CloseRequested,
}

fn items_at_path<'a>(model: &'a MenuModel, path: &[String]) -> Option<&'a [MenuItem]> {
    let mut items = model.items.as_slice();
    for id in path {
        let item = items
            .iter()
            .find(|item| item.id == *id && item.kind == MenuItemKind::Submenu)?;
        items = item.children.as_slice();
    }
    Some(items)
}

fn first_actionable_id(items: &[MenuItem], query: &str) -> Option<String> {
    filter_visible_rows(items, query)
        .into_iter()
        .find(|item| item.is_actionable())
        .map(|item| item.id.clone())
}

fn append_bounded(target: &mut String, addition: &str, max_chars: usize) {
    let current = target.chars().count();
    if current >= max_chars {
        return;
    }
    target.extend(addition.chars().take(max_chars - current));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ConfirmationKind, MenuAction, MenuItem, MenuModel};

    fn model() -> MenuModel {
        MenuModel::new(
            "demo",
            "Demo",
            vec![
                MenuItem::section("section:apps", "Apps"),
                MenuItem::status("status:ready", "Ready"),
                MenuItem::action(
                    "terminal",
                    "Terminal",
                    MenuAction::Activate {
                        id: "app:terminal".into(),
                    },
                ),
                MenuItem::action(
                    "disabled",
                    "Disabled",
                    MenuAction::Activate {
                        id: "app:disabled".into(),
                    },
                )
                .disabled(),
                MenuItem::submenu(
                    "system",
                    "System",
                    vec![
                        MenuItem::status("status:power", "Power options"),
                        MenuItem::action(
                            "restart",
                            "Restart",
                            MenuAction::Activate {
                                id: "system:restart".into(),
                            },
                        ),
                        MenuItem::action(
                            "poweroff",
                            "Power off",
                            MenuAction::destructive(
                                "Power off?",
                                Some("Unsaved work may be lost.".into()),
                                MenuAction::Custom {
                                    kind: "system.poweroff".into(),
                                    payload: String::new(),
                                },
                            ),
                        ),
                    ],
                ),
                MenuItem::action(
                    "music",
                    "Music",
                    MenuAction::Activate {
                        id: "app:music".into(),
                    },
                ),
            ],
        )
    }

    #[test]
    fn up_down_skip_non_actionable_rows_and_wrap() {
        let model = model();
        let mut state = MenuState::new(&model);
        assert_eq!(state.selected_id.as_deref(), Some("terminal"));

        state.handle(&model, SemanticInput::Down);
        assert_eq!(state.selected_id.as_deref(), Some("system"));
        state.handle(&model, SemanticInput::Down);
        assert_eq!(state.selected_id.as_deref(), Some("music"));
        state.handle(&model, SemanticInput::Down);
        assert_eq!(state.selected_id.as_deref(), Some("terminal"));
        state.handle(&model, SemanticInput::Up);
        assert_eq!(state.selected_id.as_deref(), Some("music"));
    }

    #[test]
    fn submenu_round_trip_restores_parent_semantic_identity() {
        let model = model();
        let mut state = MenuState::new(&model);
        state.selected_id = Some("system".into());

        let entered = state.handle(&model, SemanticInput::Right);
        assert_eq!(state.path, vec!["system"]);
        assert_eq!(state.selected_id.as_deref(), Some("restart"));
        assert!(matches!(
            entered,
            NavigationOutcome::SubmenuEntered { ref item_id, .. } if item_id == "system"
        ));

        state.handle(&model, SemanticInput::Down);
        assert_eq!(state.selected_id.as_deref(), Some("poweroff"));
        state.handle(&model, SemanticInput::Left);
        assert!(state.path.is_empty());
        assert_eq!(state.selected_id.as_deref(), Some("system"));
    }

    #[test]
    fn back_and_escape_walk_hierarchy_then_close() {
        let model = model();
        let mut state = MenuState::new(&model);
        state.selected_id = Some("system".into());
        state.handle(&model, SemanticInput::Right);

        assert!(matches!(
            state.handle(&model, SemanticInput::Escape),
            NavigationOutcome::SubmenuExited { .. }
        ));
        assert_eq!(
            state.handle(&model, SemanticInput::Back),
            NavigationOutcome::CloseRequested
        );
    }

    #[test]
    fn activation_returns_data_and_never_activates_status_rows() {
        let model = model();
        let mut state = MenuState::new(&model);
        let outcome = state.handle(&model, SemanticInput::Activate);
        assert_eq!(
            outcome,
            NavigationOutcome::ActionRequested {
                item_id: "terminal".into(),
                action: MenuAction::Activate {
                    id: "app:terminal".into(),
                },
            }
        );

        state.path = vec!["system".into()];
        state.selected_id = Some("status:power".into());
        let outcome = state.handle(&model, SemanticInput::Activate);
        assert_eq!(state.selected_id.as_deref(), Some("status:power"));
        assert_eq!(outcome, NavigationOutcome::Noop);
    }

    #[test]
    fn destructive_activation_requests_confirmation() {
        let model = model();
        let mut state = MenuState {
            selected_id: Some("poweroff".into()),
            path: vec!["system".into()],
            query: String::new(),
        };
        let outcome = state.handle(&model, SemanticInput::Activate);
        match outcome {
            NavigationOutcome::ConfirmationRequested {
                item_id,
                confirmation,
                action,
            } => {
                assert_eq!(item_id, "poweroff");
                assert_eq!(confirmation.kind, ConfirmationKind::Destructive);
                assert!(matches!(action, MenuAction::Custom { .. }));
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }

    #[test]
    fn filtering_preserves_selection_by_identity_when_possible() {
        let model = model();
        let mut state = MenuState::new(&model);
        state.selected_id = Some("music".into());

        state.handle(&model, SemanticInput::Text("mus".into()));
        assert_eq!(state.selected_id.as_deref(), Some("music"));

        state.handle(&model, SemanticInput::Backspace);
        state.handle(&model, SemanticInput::Backspace);
        state.handle(&model, SemanticInput::Backspace);
        assert_eq!(state.selected_id.as_deref(), Some("music"));

        state.handle(&model, SemanticInput::Text("term".into()));
        assert_eq!(state.selected_id.as_deref(), Some("terminal"));
    }

    #[test]
    fn backspace_is_unicode_safe() {
        let model = model();
        let mut state = MenuState::new(&model);
        state.handle(&model, SemanticInput::Text("é".into()));
        assert_eq!(state.query, "é");
        state.handle(&model, SemanticInput::Backspace);
        assert!(state.query.is_empty());
    }

    #[test]
    fn stale_paths_are_repaired_without_row_indices() {
        let model = model();
        let mut state = MenuState {
            selected_id: Some("missing".into()),
            path: vec!["missing-submenu".into()],
            query: String::new(),
        };
        state.normalize(&model);
        assert!(state.path.is_empty());
        assert_eq!(state.selected_id.as_deref(), Some("terminal"));
    }
}
