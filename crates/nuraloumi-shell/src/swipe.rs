//! Renderer-neutral, explicitly enabled horizontal list dismissal.
//! The caller owns provider effects; an absent handler is a local hide only.
use std::collections::BTreeSet;

pub const CAPTURE_PX: f64 = 14.0;
pub const COMMIT_FRACTION: f64 = 0.35;

pub type SwipeHandler<'a, E> = dyn FnMut(&str, SwipeDirection) -> Result<(), E> + 'a;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SwipeDirection {
    Left,
    Right,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SwipeOutcome {
    None,
    Captured {
        id: String,
        offset: f64,
    },
    Dismiss {
        id: String,
        direction: SwipeDirection,
    },
    Cancel,
}

#[derive(Clone, Debug)]
struct Contact {
    touch_id: i32,
    item_id: String,
    start_x: f64,
    start_y: f64,
    offset: f64,
    captured: bool,
    vertical: bool,
}

#[derive(Default, Debug)]
pub struct Swipe {
    contact: Option<Contact>,
    pub hidden: BTreeSet<String>,
    suppress_click: bool,
}

impl Swipe {
    pub fn begin(&mut self, touch_id: i32, item: Option<&str>, x: f64, y: f64) {
        self.contact = item
            .filter(|_| x.is_finite() && y.is_finite())
            .map(|id| Contact {
                touch_id,
                item_id: id.into(),
                start_x: x,
                start_y: y,
                offset: 0.0,
                captured: false,
                vertical: false,
            });
        self.suppress_click = false;
    }

    pub fn move_to(&mut self, touch_id: i32, x: f64, y: f64) -> SwipeOutcome {
        let Some(c) = self.contact.as_mut().filter(|c| c.touch_id == touch_id) else {
            return SwipeOutcome::None;
        };
        if !x.is_finite() || !y.is_finite() {
            self.cancel();
            return SwipeOutcome::Cancel;
        }
        let dx = x - c.start_x;
        let dy = y - c.start_y;
        if !c.captured && !c.vertical {
            if dy.abs() >= CAPTURE_PX && dy.abs() > dx.abs() {
                c.vertical = true;
            } else if dx.abs() >= CAPTURE_PX && dx.abs() > dy.abs() * 1.25 {
                c.captured = true;
                self.suppress_click = true;
            }
        }
        if c.captured {
            c.offset = dx;
            SwipeOutcome::Captured {
                id: c.item_id.clone(),
                offset: dx,
            }
        } else {
            SwipeOutcome::None
        }
    }

    pub fn release(&mut self, touch_id: i32, width: f64, still_present: bool) -> SwipeOutcome {
        let Some(c) = self.contact.take() else {
            return SwipeOutcome::None;
        };
        if c.touch_id != touch_id
            || !c.captured
            || !still_present
            || !width.is_finite()
            || width <= 0.0
        {
            return if c.captured {
                SwipeOutcome::Cancel
            } else {
                SwipeOutcome::None
            };
        }
        if c.offset.abs() < (width * COMMIT_FRACTION).max(48.0) {
            return SwipeOutcome::Cancel;
        }
        let direction = if c.offset < 0.0 {
            SwipeDirection::Left
        } else {
            SwipeDirection::Right
        };
        SwipeOutcome::Dismiss {
            id: c.item_id,
            direction,
        }
    }

    pub fn hide(&mut self, id: &str) {
        self.hidden.insert(id.into());
    }
    /// Invoke an optional typed action; failure retains the row for retry.
    /// Callers verify provider acknowledgements before durable removal.
    pub fn commit_with<E>(
        &mut self,
        id: &str,
        direction: SwipeDirection,
        handler: Option<&mut SwipeHandler<'_, E>>,
    ) -> Result<(), E> {
        if let Some(handler) = handler {
            handler(id, direction)?;
        }
        self.hide(id);
        Ok(())
    }
    pub fn restore(&mut self, id: &str) {
        self.hidden.remove(id);
    }
    /// Hidden IDs persist through refresh while the provider still reports them.
    /// Removed IDs are forgotten, so a future reused ID may be shown again.
    pub fn reconcile(&mut self, present: impl IntoIterator<Item = String>) {
        let present: BTreeSet<_> = present.into_iter().collect();
        self.hidden.retain(|id| present.contains(id));
        if self
            .contact
            .as_ref()
            .is_some_and(|c| !present.contains(&c.item_id))
        {
            self.cancel();
        }
    }
    pub fn cancel(&mut self) {
        self.contact = None;
        self.suppress_click = true;
    }
    pub fn captured(&self) -> Option<(&str, f64)> {
        self.contact
            .as_ref()
            .filter(|c| c.captured)
            .map(|c| (c.item_id.as_str(), c.offset))
    }
    pub fn suppress_click(&self) -> bool {
        self.suppress_click
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn drag(dx: f64, dy: f64) -> (Swipe, SwipeOutcome) {
        let mut s = Swipe::default();
        s.begin(1, Some("n1"), 50.0, 50.0);
        s.move_to(1, 50.0 + dx, 50.0 + dy);
        let o = s.release(1, 100.0, true);
        (s, o)
    }
    #[test]
    fn symmetric_actionless_commit() {
        for (dx, direction) in [(-75.0, SwipeDirection::Left), (75.0, SwipeDirection::Right)] {
            let (mut s, o) = drag(dx, 0.0);
            assert_eq!(
                o,
                SwipeOutcome::Dismiss {
                    id: "n1".into(),
                    direction
                }
            );
            s.hide("n1");
            assert!(s.hidden.contains("n1"));
        }
    }
    #[test]
    fn undershoot_and_vertical_are_not_dismissals() {
        assert_eq!(drag(25.0, 0.0).1, SwipeOutcome::Cancel);
        assert_eq!(drag(4.0, 60.0).1, SwipeOutcome::None);
    }
    #[test]
    fn contact_conflict_and_stale_id() {
        let mut s = Swipe::default();
        s.begin(1, Some("n1"), 0.0, 0.0);
        s.move_to(1, -60.0, 0.0);
        s.cancel();
        assert!(s.suppress_click());
        assert_eq!(s.release(1, 100.0, true), SwipeOutcome::None);
        s.begin(2, Some("n1"), 0.0, 0.0);
        s.move_to(2, 60.0, 0.0);
        assert_eq!(s.release(2, 100.0, false), SwipeOutcome::Cancel);
    }
    #[test]
    fn optional_typed_handler_and_failure_retry() {
        let mut swipe = Swipe::default();
        let mut events = Vec::new();
        let mut cb = |id: &str, direction| -> Result<(), &'static str> {
            events.push((id.to_owned(), direction));
            Ok(())
        };
        swipe
            .commit_with("a", SwipeDirection::Right, Some(&mut cb))
            .unwrap();
        assert_eq!(events, [("a".into(), SwipeDirection::Right)]);
        swipe
            .commit_with::<&str>("b", SwipeDirection::Left, None)
            .unwrap();
        let mut fail = |_: &str, _: SwipeDirection| Err::<(), _>("offline");
        assert_eq!(
            swipe.commit_with("c", SwipeDirection::Left, Some(&mut fail)),
            Err("offline")
        );
        assert!(!swipe.hidden.contains("c"));
        swipe
            .commit_with::<&str>("c", SwipeDirection::Left, None)
            .unwrap();
        assert!(swipe.hidden.contains("c"));
    }
    #[test]
    fn refresh_retains_only_current_hidden_ids() {
        let mut s = Swipe::default();
        s.hide("n1");
        s.hide("n2");
        s.reconcile(["n2".into(), "n3".into()]);
        assert_eq!(s.hidden, BTreeSet::from(["n2".into()]));
        s.restore("n2");
        assert!(s.hidden.is_empty());
    }
}
