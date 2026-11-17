//! Window-independent full-player controller. Deliberately no layer-shell fallback:
//! a true compositor-managed xdg_toplevel is required before presenting UI.
//! No playback pipeline is created here; every operation targets playerd.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerWindowAction {
    Toggle,
    Stop,
    Next,
    Previous,
    SeekTo(u64),
    SeekRelative(i64),
    CloseWindow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowBackend {
    XdgToplevel,
    LayerShellOnly,
}

pub fn require_managed_window(backend: WindowBackend) -> Result<(), &'static str> {
    match backend {
        WindowBackend::XdgToplevel => Ok(()),
        WindowBackend::LayerShellOnly => {
            Err("xdg_toplevel backend missing: cannot present a Sway-managed player window")
        }
    }
}

/// Map a high-precision seek position to milliseconds without overflowing;
/// unknown/zero duration means that dragging is disabled, not a seek to 0.
pub fn seek_from_slider(x: f64, width: f64, duration_ms: u64) -> Option<u64> {
    if !x.is_finite() || !width.is_finite() || width < 48.0 || duration_ms == 0 {
        return None;
    }
    Some(((x / width).clamp(0.0, 1.0) * duration_ms as f64).round() as u64)
}

pub fn slider_fraction(position_ms: u64, duration_ms: u64) -> f64 {
    if duration_ms == 0 {
        return 0.0;
    }
    position_ms.min(duration_ms) as f64 / duration_ms as f64
}

/// Keys are resolved outside the paint path. Closing the view must not stop audio.
pub fn shortcut(key: &str, position_ms: u64, duration_ms: u64) -> Option<PlayerWindowAction> {
    match key {
        "space" => Some(PlayerWindowAction::Toggle),
        "left" => Some(PlayerWindowAction::SeekRelative(-15_000)),
        "right" => Some(PlayerWindowAction::SeekRelative(15_000)),
        "home" if duration_ms > 0 => Some(PlayerWindowAction::SeekTo(0)),
        "end" if duration_ms > 0 => Some(PlayerWindowAction::SeekTo(duration_ms)),
        "n" => Some(PlayerWindowAction::Next),
        "p" => Some(PlayerWindowAction::Previous),
        "s" => Some(PlayerWindowAction::Stop),
        "escape" | "q" => Some(PlayerWindowAction::CloseWindow),
        "pageup" if duration_ms > 0 => Some(PlayerWindowAction::SeekTo(
            position_ms.saturating_add(60_000).min(duration_ms),
        )),
        "pagedown" if duration_ms > 0 => Some(PlayerWindowAction::SeekTo(
            position_ms.saturating_sub(60_000),
        )),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn absent_xdg_cannot_masquerade_as_desktop_window() {
        assert!(require_managed_window(WindowBackend::LayerShellOnly).is_err());
        assert!(require_managed_window(WindowBackend::XdgToplevel).is_ok());
    }
    #[test]
    fn seek_edges_and_unknown_length() {
        assert_eq!(seek_from_slider(0.0, 480.0, 123_456), Some(0));
        assert_eq!(seek_from_slider(480.0, 480.0, 123_456), Some(123_456));
        assert_eq!(seek_from_slider(240.0, 480.0, 123_456), Some(61_728));
        assert_eq!(seek_from_slider(-50.0, 480.0, 1000), Some(0));
        assert_eq!(seek_from_slider(999.0, 480.0, 1000), Some(1000));
        assert_eq!(seek_from_slider(f64::NAN, 480.0, 1000), None);
        assert_eq!(seek_from_slider(20.0, 20.0, 1000), None);
        assert_eq!(seek_from_slider(200.0, 480.0, 0), None);
        assert_eq!(slider_fraction(u64::MAX, 100), 1.0);
    }
    #[test]
    fn window_close_never_terminates_daemon() {
        assert_eq!(
            shortcut("escape", 0, 0),
            Some(PlayerWindowAction::CloseWindow)
        );
        assert_eq!(shortcut("q", 0, 0), Some(PlayerWindowAction::CloseWindow));
        assert_eq!(
            shortcut("left", 0, 0),
            Some(PlayerWindowAction::SeekRelative(-15_000))
        );
        assert_eq!(shortcut("end", 42, 0), None);
    }
}
