//! Pure, bounded one-shot toast lifecycle.
//!
//! This file deliberately has no notification subscriber or daemon. A shell
//! caller supplies an existing NotificationEntry and an authoritative
//! notification-center-open flag; a separate binary owns the Wayland surface.

use nuraloumi_providers::notifications::NotificationEntry;
use nuraloumi_wayland::{Key, PlatformEvent};

pub const DEFAULT_TIMEOUT_MS: u64 = 4_500;
pub const MIN_TIMEOUT_MS: u64 = 500;
pub const MAX_TIMEOUT_MS: u64 = 10_000;

const MAX_APP_CHARS: usize = 48;
const MAX_SUMMARY_CHARS: usize = 120;
const MAX_BODY_CHARS: usize = 240;

/// Monotonic milliseconds come from the caller; no wall-clock dependency.
#[derive(Debug)]
pub struct Toast {
    pub notification: NotificationEntry,
    deadline_ms: u64,
    closed: bool,
    pointer_button: Option<u32>,
    touch_id: Option<i32>,
}

impl Toast {
    /// Returns None when the notification center already owns presentation.
    pub fn new(
        mut notification: NotificationEntry,
        opened_at_ms: u64,
        requested_timeout_ms: u64,
        notification_center_open: bool,
    ) -> Option<Self> {
        if notification_center_open {
            return None;
        }
        notification.app = bounded_line(&notification.app, MAX_APP_CHARS);
        notification.summary = bounded_line(&notification.summary, MAX_SUMMARY_CHARS);
        notification.body = bounded_line(&notification.body, MAX_BODY_CHARS);
        if notification.summary.is_empty() {
            notification.summary = "Notification".to_owned();
        }
        let deadline_ms =
            opened_at_ms.saturating_add(requested_timeout_ms.clamp(MIN_TIMEOUT_MS, MAX_TIMEOUT_MS));
        Some(Self {
            notification,
            deadline_ms,
            closed: false,
            pointer_button: None,
            touch_id: None,
        })
    }

    pub fn remaining_ms(&self, now_ms: u64) -> Option<u64> {
        if self.closed || now_ms >= self.deadline_ms {
            None
        } else {
            Some(self.deadline_ms - now_ms)
        }
    }

    /// Pointer and touch activation occurs only on matching release.
    /// Touch cancel and movement off the toast invalidate the gesture.
    pub fn handle(&mut self, event: PlatformEvent, now_ms: u64, extent: (u32, u32)) -> bool {
        if self.remaining_ms(now_ms).is_none() {
            self.closed = true;
            return true;
        }
        match event {
            PlatformEvent::Close => self.closed = true,
            PlatformEvent::Key {
                key: Key::Escape | Key::Enter | Key::Space,
                pressed: true,
            } => self.closed = true,
            PlatformEvent::PointerButton {
                x,
                y,
                pressed,
                button,
            } if button == 0x110 => {
                if pressed {
                    self.pointer_button = inside(x, y, extent).then_some(button);
                } else {
                    if self.pointer_button == Some(button) && inside(x, y, extent) {
                        self.closed = true;
                    }
                    self.pointer_button = None;
                }
            }
            PlatformEvent::PointerMove { x, y } => {
                if !inside(x, y, extent) {
                    self.pointer_button = None;
                }
            }
            PlatformEvent::PointerLeave => self.pointer_button = None,
            PlatformEvent::TouchDown { id, x, y } => {
                // A second finger invalidates the first gesture instead of
                // accidentally dismissing the toast on an unrelated release.
                self.touch_id = if self.touch_id.is_some() {
                    None
                } else {
                    inside(x, y, extent).then_some(id)
                };
            }
            PlatformEvent::TouchMotion { id, x, y } => {
                if self.touch_id == Some(id) && !inside(x, y, extent) {
                    self.touch_id = None;
                }
            }
            PlatformEvent::TouchUp { id } => {
                if self.touch_id == Some(id) {
                    self.closed = true;
                }
                self.touch_id = None;
            }
            PlatformEvent::TouchCancel { ids } => {
                if self.touch_id.is_some_and(|id| ids.contains(&id)) {
                    self.touch_id = None;
                }
            }
            _ => {}
        }
        self.closed
    }
}

fn inside(x: f64, y: f64, (width, height): (u32, u32)) -> bool {
    x.is_finite()
        && y.is_finite()
        && x >= 0.0
        && y >= 0.0
        && x < f64::from(width)
        && y < f64::from(height)
}

fn bounded_line(text: &str, max_chars: usize) -> String {
    let mut result = String::new();
    let mut pending_space = false;
    let mut count = 0;
    let mut truncated = false;
    for c in text.chars() {
        if c.is_whitespace() || c.is_control() {
            pending_space = !result.is_empty();
            continue;
        }
        if pending_space && count < max_chars {
            result.push(' ');
            count += 1;
        }
        pending_space = false;
        if count == max_chars {
            truncated = true;
            break;
        }
        result.push(c);
        count += 1;
    }
    if truncated {
        result.pop();
        result.push('…');
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> NotificationEntry {
        NotificationEntry {
            id: 12,
            app: "Test".into(),
            summary: "Hello".into(),
            body: "Text".into(),
            actions: vec![],
            default_action: None,
        }
    }

    fn toast() -> Toast {
        Toast::new(entry(), 100, 1500, false).unwrap()
    }

    const AREA: (u32, u32) = (320, 180);

    #[test]
    fn normalizes_and_bounds_unicode_text() {
        let mut n = entry();
        n.app = " a\nb\u{0007}\t  c ".to_owned();
        n.summary = "🐈".repeat(400);
        n.body = "ñ".repeat(900);
        let t = Toast::new(n, 0, DEFAULT_TIMEOUT_MS, false).unwrap();
        assert_eq!(t.notification.app, "a b c");
        assert_eq!(t.notification.summary.chars().count(), 120);
        assert!(t.notification.summary.ends_with('…'));
        assert_eq!(t.notification.body.chars().count(), 240);
        assert_eq!(t.notification.id, 12);
        assert!(t.notification.actions.is_empty());
    }

    #[test]
    fn empty_summary_gets_a_safe_fallback() {
        let mut n = entry();
        n.summary = "\n \t".into();
        let t = Toast::new(n, 0, 4500, false).unwrap();
        assert_eq!(t.notification.summary, "Notification");
    }

    #[test]
    fn deadline_is_bounded_monotonic_and_overflow_safe() {
        let t = Toast::new(entry(), 100, 0, false).unwrap();
        assert_eq!(t.remaining_ms(599), Some(1));
        assert_eq!(t.remaining_ms(600), None);
        let t = Toast::new(entry(), 5, u64::MAX, false).unwrap();
        assert_eq!(t.remaining_ms(10_004), Some(1));
        assert_eq!(t.remaining_ms(10_005), None);
        let t = Toast::new(entry(), u64::MAX - 1, 2_000, false).unwrap();
        assert_eq!(t.remaining_ms(u64::MAX - 1), Some(1));
        assert_eq!(t.remaining_ms(u64::MAX), None);
    }

    #[test]
    fn center_coexistence_suppresses_initial_toast() {
        assert!(Toast::new(entry(), 0, 4000, true).is_none());
        assert!(Toast::new(entry(), 0, 4000, false).is_some());
    }

    #[test]
    fn pointer_requires_matching_left_button_release_inside() {
        let mut t = toast();
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: false,
                button: 0x110
            },
            101,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: true,
                button: 0x111
            },
            102,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: false,
                button: 0x111
            },
            103,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: true,
                button: 0x110
            },
            104,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 400.0,
                y: 20.0,
                pressed: false,
                button: 0x110
            },
            105,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: true,
                button: 0x110
            },
            106,
            AREA
        ));
        assert!(t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: false,
                button: 0x110
            },
            107,
            AREA
        ));
    }

    #[test]
    fn touch_requires_own_release_without_drag_cancel_or_second_finger() {
        let mut t = toast();
        assert!(!t.handle(
            PlatformEvent::TouchDown {
                id: 1,
                x: 30.0,
                y: 30.0
            },
            101,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::TouchMotion {
                id: 1,
                x: -1.0,
                y: 30.0
            },
            102,
            AREA
        ));
        assert!(!t.handle(PlatformEvent::TouchUp { id: 1 }, 103, AREA));
        assert!(!t.handle(
            PlatformEvent::TouchDown {
                id: 2,
                x: 30.0,
                y: 30.0
            },
            104,
            AREA
        ));
        assert!(!t.handle(PlatformEvent::TouchCancel { ids: vec![2] }, 105, AREA));
        assert!(!t.handle(PlatformEvent::TouchUp { id: 2 }, 106, AREA));
        assert!(!t.handle(
            PlatformEvent::TouchDown {
                id: 3,
                x: 30.0,
                y: 30.0
            },
            107,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::TouchDown {
                id: 4,
                x: 30.0,
                y: 30.0
            },
            108,
            AREA
        ));
        assert!(!t.handle(PlatformEvent::TouchUp { id: 3 }, 109, AREA));
        assert!(!t.handle(PlatformEvent::TouchUp { id: 4 }, 110, AREA));
        assert!(!t.handle(
            PlatformEvent::TouchDown {
                id: 5,
                x: 30.0,
                y: 30.0
            },
            111,
            AREA
        ));
        assert!(!t.handle(PlatformEvent::TouchUp { id: 7 }, 112, AREA));
        assert!(!t.handle(PlatformEvent::TouchCancel { ids: vec![5] }, 112, AREA));
        assert!(!t.handle(
            PlatformEvent::TouchDown {
                id: 6,
                x: 30.0,
                y: 30.0
            },
            113,
            AREA
        ));
        assert!(t.handle(PlatformEvent::TouchUp { id: 6 }, 114, AREA));
    }

    #[test]
    fn keyboard_dismisses_only_on_key_press_and_expiry_precedes_input() {
        let mut t = toast();
        assert!(!t.handle(
            PlatformEvent::Key {
                key: Key::Escape,
                pressed: false
            },
            101,
            AREA
        ));
        assert!(t.handle(
            PlatformEvent::Key {
                key: Key::Escape,
                pressed: true
            },
            102,
            AREA
        ));
        let mut t = toast();
        assert!(t.handle(
            PlatformEvent::Key {
                key: Key::Enter,
                pressed: true
            },
            103,
            AREA
        ));
        let mut t = toast();
        assert!(t.handle(
            PlatformEvent::Key {
                key: Key::Space,
                pressed: true
            },
            104,
            AREA
        ));
        let mut t = toast();
        assert!(t.handle(
            PlatformEvent::TouchDown {
                id: 1,
                x: 20.0,
                y: 20.0
            },
            1600,
            AREA
        ));
    }

    #[test]
    fn pointer_leave_and_nonfinite_coordinates_are_not_activation() {
        let mut t = toast();
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: f64::NAN,
                y: 0.0,
                pressed: true,
                button: 0x110
            },
            101,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: false,
                button: 0x110
            },
            102,
            AREA
        ));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: true,
                button: 0x110
            },
            103,
            AREA
        ));
        assert!(!t.handle(PlatformEvent::PointerLeave, 104, AREA));
        assert!(!t.handle(
            PlatformEvent::PointerButton {
                x: 20.0,
                y: 20.0,
                pressed: false,
                button: 0x110
            },
            105,
            AREA
        ));
    }
}
