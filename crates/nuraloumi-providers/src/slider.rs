//! Debounced binary hardware-state observer. Source and polarity are deliberately
//! supplied by the caller; neither is inferred from the SL101 product name.
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SliderState {
    Open,
    Closed,
}

impl SliderState {
    pub fn summary(self) -> &'static str {
        match self {
            Self::Open => "Slider opened",
            Self::Closed => "Slider closed",
        }
    }
}

/// `None` means the sensor cannot be read, including after hot-unplug/resume.
/// A new baseline is mandatory after any loss, so reconnection never notifies.
pub struct SliderObserver {
    stable: Option<SliderState>,
    pending: Option<(SliderState, Instant)>,
    debounce: Duration,
}

impl SliderObserver {
    pub fn new(debounce: Duration) -> Self {
        Self {
            stable: None,
            pending: None,
            debounce,
        }
    }

    pub fn observe(&mut self, sample: Option<SliderState>, now: Instant) -> Option<SliderState> {
        let Some(state) = sample else {
            self.stable = None;
            self.pending = None;
            return None;
        };
        if self.stable == Some(state) {
            self.pending = None;
            return None;
        }
        match self.pending {
            Some((candidate, since)) if candidate == state => {
                if now.saturating_duration_since(since) < self.debounce {
                    return None;
                }
                let old = self.stable.replace(state);
                self.pending = None;
                old.map(|_| state) // boot snapshot/reconnect never emits
            }
            _ => {
                self.pending = Some((state, now));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transitions_bounce_duplicates_and_initial_snapshot() {
        let t = Instant::now();
        let mut observer = SliderObserver::new(Duration::from_millis(250));
        assert_eq!(observer.observe(Some(SliderState::Closed), t), None);
        assert_eq!(
            observer.observe(Some(SliderState::Closed), t + Duration::from_millis(250)),
            None
        );
        assert_eq!(
            observer.observe(Some(SliderState::Open), t + Duration::from_millis(300)),
            None
        );
        assert_eq!(
            observer.observe(Some(SliderState::Closed), t + Duration::from_millis(330)),
            None
        );
        assert_eq!(
            observer.observe(Some(SliderState::Open), t + Duration::from_millis(350)),
            None
        );
        assert_eq!(
            observer.observe(Some(SliderState::Open), t + Duration::from_millis(599)),
            None
        );
        assert_eq!(
            observer.observe(Some(SliderState::Open), t + Duration::from_millis(600)),
            Some(SliderState::Open)
        );
        assert_eq!(
            observer.observe(Some(SliderState::Open), t + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            observer.observe(Some(SliderState::Closed), t + Duration::from_secs(3)),
            None
        );
        assert_eq!(
            observer.observe(Some(SliderState::Closed), t + Duration::from_millis(3250)),
            Some(SliderState::Closed)
        );
    }
    #[test]
    fn missing_sensor_requires_fresh_baseline() {
        let t = Instant::now();
        let mut o = SliderObserver::new(Duration::from_millis(100));
        o.observe(Some(SliderState::Open), t);
        o.observe(Some(SliderState::Open), t + Duration::from_millis(100));
        assert_eq!(o.observe(None, t + Duration::from_millis(120)), None);
        assert_eq!(
            o.observe(Some(SliderState::Closed), t + Duration::from_millis(150)),
            None
        );
        assert_eq!(
            o.observe(Some(SliderState::Closed), t + Duration::from_millis(250)),
            None
        );
        assert_eq!(
            o.observe(Some(SliderState::Open), t + Duration::from_millis(300)),
            None
        );
        assert_eq!(
            o.observe(Some(SliderState::Open), t + Duration::from_millis(400)),
            Some(SliderState::Open)
        );
    }
    #[test]
    fn unavailable_forever_is_silent() {
        let mut o = SliderObserver::new(Duration::from_millis(200));
        let t = Instant::now();
        for i in 0..100 {
            assert_eq!(o.observe(None, t + Duration::from_millis(i * 100)), None);
        }
    }
}
