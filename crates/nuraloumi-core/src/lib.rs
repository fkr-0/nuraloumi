//! Renderer-neutral menu semantics for NuraLoumi.
//!
//! This crate owns the stable menu data model, validation, semantic navigation,
//! search and quick-select helpers, design tokens, and pure motion primitives.
//! It intentionally has no dependency on a GUI toolkit, window system, async
//! runtime, system service, or timer implementation.

pub mod action_ids;
pub mod model;
pub mod motion;
pub mod navigation;
pub mod search;
pub mod theme;
pub mod validation;

pub use model::{Confirmation, ConfirmationKind, MenuAction, MenuItem, MenuItemKind, MenuModel};
pub use motion::{
    ease_in_cubic, ease_in_out_cubic, ease_out_cubic, enter_transition, exit_transition, fade_in,
    fade_out, normalized_progress, reduced_motion, MotionClass, MotionMode, Transition,
};
pub use navigation::{MenuState, NavigationOutcome, SemanticInput};
pub use search::{
    filter_visible_rows, matches_query, quick_select_labels, QuickSelectLabel,
    QUICK_SELECT_ALPHABET,
};
pub use theme::{
    BorderTokens, ElevationTokens, HitTargetTokens, IconTokens, OpacityTokens, ShadowToken,
    SpacingTokens, SurfaceColors, TextColors, ThemeTokens, TypeTokens, UiColor, UiMetrics,
    DARK_THEME, LIGHT_THEME, UI_METRICS,
};
pub use validation::{ValidationError, ValidationLimits};

/// Validate a menu with the production default limits.
pub fn validate_menu(model: &MenuModel) -> Result<(), ValidationError> {
    ValidationLimits::default().validate(model)
}
