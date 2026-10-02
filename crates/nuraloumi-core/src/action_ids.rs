//! Stable semantic action identifiers shared by menu producers and executors.
//!
//! These strings are part of NuraLoumi's renderer-neutral semantic contract:
//! providers and shells may evolve independently, but the meaning of a
//! built-in action must not drift because two binaries happened to spell an
//! identifier differently.
//!
//! They deliberately remain `&str` constants rather than a serialized enum so
//! existing menu fixtures and wire representations stay source-compatible.

/// IDs carried by [`crate::MenuAction::Activate`],
/// [`crate::MenuAction::Toggle`], and [`crate::MenuAction::Adjust`].
pub mod id {
    pub const NETWORK_WIFI: &str = "network.wifi";
    pub const NETWORK_RESCAN: &str = "network.rescan";
    pub const BLUETOOTH_RADIO: &str = "bluetooth.radio";
    pub const AUDIO_MUTE: &str = "audio.mute";
    pub const AUDIO_VOLUME: &str = "audio.volume";
    pub const SYSTEM_BRIGHTNESS: &str = "system.brightness";
    pub const SYSTEM_SUSPEND: &str = "system.suspend";
    pub const SYSTEM_RESTART: &str = "system.restart";
    pub const SYSTEM_POWEROFF: &str = "system.poweroff";

    /// Prefix for window fullscreen toggle IDs. The opaque compositor
    /// toplevel identity follows the colon.
    pub const WINDOW_FULLSCREEN_PREFIX: &str = "window.fullscreen:";

    pub const BUILTIN: &[&str] = &[
        NETWORK_WIFI,
        NETWORK_RESCAN,
        BLUETOOTH_RADIO,
        AUDIO_MUTE,
        AUDIO_VOLUME,
        SYSTEM_BRIGHTNESS,
        SYSTEM_SUSPEND,
        SYSTEM_RESTART,
        SYSTEM_POWEROFF,
    ];
}

/// Kinds carried by [`crate::MenuAction::Custom`].
pub mod custom_kind {
    pub const NETWORK_CONNECT: &str = "network.connect";
    pub const BLUETOOTH_CONNECT: &str = "bluetooth.connect";
    pub const BLUETOOTH_DISCONNECT: &str = "bluetooth.disconnect";
    pub const MENU_OPEN: &str = "menu.open";
    pub const TASK_INSPECT: &str = "task.inspect";
    pub const WINDOW_FOCUS: &str = "window.focus";
    pub const WINDOW_CLOSE: &str = "window.close";

    pub const BUILTIN: &[&str] = &[
        NETWORK_CONNECT,
        BLUETOOTH_CONNECT,
        BLUETOOTH_DISCONNECT,
        MENU_OPEN,
        TASK_INSPECT,
        WINDOW_FOCUS,
        WINDOW_CLOSE,
    ];
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::{custom_kind, id};

    #[test]
    fn builtin_semantic_identifiers_are_nonempty_unique_and_namespaced() {
        for values in [id::BUILTIN, custom_kind::BUILTIN] {
            let mut seen = BTreeSet::new();
            for value in values {
                assert!(!value.is_empty());
                assert!(!value.contains('\0'));
                assert!(value.contains('.'), "{value:?} is not namespaced");
                assert!(
                    seen.insert(*value),
                    "duplicate semantic identifier {value:?}"
                );
            }
        }
    }

    #[test]
    fn fullscreen_prefix_is_namespaced_and_payload_separated() {
        assert!(id::WINDOW_FULLSCREEN_PREFIX.starts_with("window."));
        assert!(id::WINDOW_FULLSCREEN_PREFIX.ends_with(':'));
    }
}
