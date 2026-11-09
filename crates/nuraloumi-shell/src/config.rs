//! Versioned, extensible shell configuration sections.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Per-surface values are deliberately extensible: unknown names and keys survive
/// deserialize/serialize round trips and are ignored by consumers.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SurfaceConfig {
    pub show_album_art: Option<bool>,
    pub refresh_interval_ms: Option<u64>,
    pub ical_path: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub terminal: String,
    pub editor: String,
    pub weather_url: String,
    pub weather_cache_seconds: u64,
    pub media: MediaProviderConfig,
    pub resources: ResourcesProviderConfig,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}
impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            terminal: "foot".into(),
            editor: "nvim".into(),
            weather_url: "https://wttr.in/?format=j1".into(),
            weather_cache_seconds: 1800,
            media: MediaProviderConfig::default(),
            resources: ResourcesProviderConfig::default(),
            extra: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::ShellConfig;

    #[test]
    fn version_and_unknown_sections_roundtrip() {
        let legacy: ShellConfig = toml::from_str("panel_height = 40").unwrap();
        assert_eq!(legacy.config_version, 0);
        assert_eq!(ShellConfig::default().config_version, 1);
        let config: ShellConfig = toml::from_str(
            "config_version = 1\nfuture_key = 7\n[surface.third_party]\ncolor = 'teal'\n",
        )
        .unwrap();
        config.validate().unwrap();
        assert_eq!(
            config.surface["third_party"].extra["color"].as_str(),
            Some("teal")
        );
        assert_eq!(config.extra["future_key"].as_integer(), Some(7));
        let restored: ShellConfig = toml::from_str(&toml::to_string(&config).unwrap()).unwrap();
        assert_eq!(restored.surface, config.surface);
        assert_eq!(restored.extra, config.extra);
    }

    #[test]
    fn rejects_future_version_invalid_slots_and_unknown_strict_fields() {
        let future: ShellConfig = toml::from_str("config_version = 2").unwrap();
        assert!(future
            .validate()
            .unwrap_err()
            .contains("unsupported config_version"));
        let bad: ShellConfig = toml::from_str(
            "[[panel.quick_launch]]\nlabel = 'Bad'\ncommand = 'foot'\ndesktop = 'foot.desktop'\n",
        )
        .unwrap();
        assert!(bad.validate().unwrap_err().contains("exactly one"));
        assert!(toml::from_str::<ShellConfig>(
            "[providers.media]\nbackend = 'playerctl'\nunknown = true\n"
        )
        .is_err());
    }

    #[test]
    fn example_is_valid_v1() {
        let example = include_str!("../../../examples/nuraloumi-config.toml");
        let config: ShellConfig = toml::from_str(example).unwrap();
        config.validate().unwrap();
        assert_eq!(config.config_version, 1);
        assert_eq!(config.panel.quick_launch.len(), 3);
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaProviderConfig {
    pub backend: String,
}
impl Default for MediaProviderConfig {
    fn default() -> Self {
        Self {
            backend: "playerctl".into(),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ResourcesProviderConfig {
    pub refresh_interval_ms: u64,
    pub top_process_count: u32,
}
impl Default for ResourcesProviderConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: 5000,
            top_process_count: 10,
        }
    }
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PanelConfig {
    pub quick_launch: Vec<QuickLaunchSlot>,
}
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct QuickLaunchSlot {
    pub label: String,
    pub glyph: String,
    pub command: Option<String>,
    pub desktop: Option<String>,
}
impl QuickLaunchSlot {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if self.label.trim().is_empty()
            || self.label.chars().count() > 64
            || self.label.contains(['\n', '\r', '\0'])
        {
            return Err("panel.quick_launch label must be 1..=64 characters on one line".into());
        }
        match (&self.command, &self.desktop) {
            (Some(cmd), None) if !cmd.trim().is_empty() && !cmd.contains(['\n', '\r', '\0']) => {
                Ok(())
            }
            (None, Some(id)) => super::validate_launcher_desktop_id(id),
            _ => Err(
                "panel.quick_launch must specify exactly one nonempty command or desktop".into(),
            ),
        }
    }
}
