//! Pure validate/plan boundary for hot configuration replacement.
//!
//! Only the Wayland owner commits a prepared plan. No render or provider work.
use crate::config_migration::{prepare_reload, read_bounded, Diagnostic, MigrationError};
use crate::{KeybindingRegistry, ShellConfig};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct PreparedConfigReload {
    pub config: ShellConfig,
    pub keybindings: KeybindingRegistry,
    pub diagnostics: Vec<Diagnostic>,
    pub source_version: u32,
    pub changed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReloadError {
    pub code: &'static str,
    pub message: String,
}

impl From<MigrationError> for ReloadError {
    fn from(error: MigrationError) -> Self {
        Self {
            code: error.code,
            message: error.message,
        }
    }
}

impl ReloadError {
    fn restart_required(properties: Vec<&'static str>) -> Self {
        Self {
            code: "restart_required",
            message: format!(
                "restart required for structural configuration: {}",
                properties.join(", ")
            ),
        }
    }
}

/// Identical XDG search priority as startup. A vanished file must not reset
/// a previously valid running configuration.
pub fn reload_source_path(explicit: Option<&Path>) -> Result<PathBuf, ReloadError> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    let root = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    if let Some(root) = root {
        for filename in ["config.toml", "config.json"] {
            let candidate = root.join("nuraloumi").join(filename);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(ReloadError {
        code: "config_missing",
        message: "no JSON/TOML configuration file found; running state unchanged".to_owned(),
    })
}

pub fn prepare_from_file(
    active: &ShellConfig,
    explicit: Option<&Path>,
    forced_reduced_motion: bool,
) -> Result<PreparedConfigReload, ReloadError> {
    let path = reload_source_path(explicit)?;
    let input = read_bounded(&path)?;
    prepare_config_reload(active, &path, &input, forced_reduced_motion)
}

/// All validation and structural checks complete before the owner commits.
/// The existing runtime configuration is never mutated by this function.
pub fn prepare_config_reload(
    active: &ShellConfig,
    path: &Path,
    input: &str,
    forced_reduced_motion: bool,
) -> Result<PreparedConfigReload, ReloadError> {
    let validated = prepare_reload(path, input)?;
    let mut next = validated.config;
    if forced_reduced_motion {
        next.reduced_motion = true;
    }
    let mut immutable = Vec::new();
    if next.panel_edge != active.panel_edge {
        immutable.push("panel_edge");
    }
    if next.panel_height != active.panel_height {
        immutable.push("panel_height");
    }
    if next.menu_width != active.menu_width {
        immutable.push("menu_width");
    }
    if next.row_height != active.row_height {
        immutable.push("row_height");
    }
    if next.panel != active.panel {
        immutable.push("panel");
    }
    if next.providers != active.providers {
        immutable.push("providers");
    }
    if next.surface != active.surface {
        immutable.push("surface");
    }
    // Backdrop creation/destruction is a Wayland surface lifecycle change.
    if next.menu_dismissal.outside_press != active.menu_dismissal.outside_press {
        immutable.push("menu_dismissal.outside_press");
    }
    if !immutable.is_empty() {
        return Err(ReloadError::restart_required(immutable));
    }
    let keybindings =
        KeybindingRegistry::from_config(&next.keybindings).map_err(|message| ReloadError {
            code: "validation_error",
            message,
        })?;
    let changed = next != *active;
    Ok(PreparedConfigReload {
        config: next,
        keybindings,
        diagnostics: validated.diagnostics,
        source_version: validated.source_version,
        changed,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;

    fn source(config: &ShellConfig) -> String {
        toml::to_string(config).expect("valid config")
    }

    #[test]
    fn invalid_candidate_never_changes_active() {
        let active = ShellConfig::default();
        let previous = active.clone();
        let error =
            prepare_config_reload(&active, Path::new("config.toml"), "panel_height = 8", false)
                .unwrap_err();
        assert_eq!(error.code, "validation_error");
        assert_eq!(active, previous);
    }

    #[test]
    fn mutable_theme_text_behavior_and_keys_can_be_staged() {
        let active = ShellConfig::default();
        let mut desired = active.clone();
        desired.theme = Theme::Light;
        desired.reduced_motion = true;
        desired.menu_dismissal.after_one_shot_action = false;
        desired
            .launcher
            .labels
            .insert("foot.desktop".into(), "Terminal".into());
        let plan =
            prepare_config_reload(&active, Path::new("config.toml"), &source(&desired), false)
                .unwrap();
        assert!(plan.changed);
        assert_eq!(plan.config, desired);
        assert_eq!(active.theme, Theme::Dark);
    }

    #[test]
    fn immutable_fields_require_restart_without_mutation() {
        let active = ShellConfig::default();
        let mut desired = active.clone();
        desired.theme = Theme::Light;
        desired.panel_height += 1;
        desired.menu_width += 1;
        desired.menu_dismissal.outside_press = !desired.menu_dismissal.outside_press;
        let error =
            prepare_config_reload(&active, Path::new("config.toml"), &source(&desired), false)
                .unwrap_err();
        assert_eq!(error.code, "restart_required");
        for field in ["panel_height", "menu_width", "menu_dismissal.outside_press"] {
            assert!(error.message.contains(field));
        }
        assert_eq!(active.theme, Theme::Dark);
    }

    #[test]
    fn forced_reduced_motion_cannot_be_disabled_by_file() {
        let active = ShellConfig {
            reduced_motion: true,
            ..ShellConfig::default()
        };
        let mut desired = active.clone();
        desired.reduced_motion = false;
        let plan = prepare_config_reload(
            &active,
            Path::new("config.json"),
            &serde_json::to_string(&desired).unwrap(),
            true,
        )
        .unwrap();
        assert!(plan.config.reduced_motion);
        assert!(!plan.changed);
    }
}
