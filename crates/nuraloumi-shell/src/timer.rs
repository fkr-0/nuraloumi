//! Pure monotonic timer and stopwatch state machine. No I/O or renderer.
use std::time::{Duration, Instant};
pub const MAX_LAPS: usize = 32;
pub const MAX_COUNTDOWN: Duration = Duration::from_secs(24 * 60 * 60);
pub const ACTIVE_TICK: Duration = Duration::from_millis(250);

pub trait Clock {
    fn now(&self) -> Duration;
}
#[derive(Debug)]
pub struct MonotonicClock {
    origin: Instant,
}
impl Default for MonotonicClock {
    fn default() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}
impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.origin.elapsed()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Stopwatch,
    Countdown(Duration),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    pub elapsed: Duration,
    pub remaining: Option<Duration>,
    pub running: bool,
    pub completed: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tick {
    pub redraw: bool,
    pub completed_now: bool,
}
pub struct Timer<C: Clock> {
    clock: C,
    kind: Kind,
    base: Duration,
    started: Option<Duration>,
    pub laps: Vec<Duration>,
    completed: bool,
    completion_pending: bool,
    last_bucket: Option<u128>,
}
impl<C: Clock> Timer<C> {
    pub fn new(clock: C, kind: Kind) -> Result<Self, String> {
        if let Kind::Countdown(d) = kind {
            if d.is_zero() || d > MAX_COUNTDOWN {
                return Err("countdown must be 1ms .. 24 hours".into());
            }
        }
        Ok(Self {
            clock,
            kind,
            base: Duration::ZERO,
            started: None,
            laps: Vec::new(),
            completed: false,
            completion_pending: false,
            last_bucket: None,
        })
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }
    pub fn running(&self) -> bool {
        self.started.is_some()
    }
    fn raw_elapsed(&self) -> Duration {
        self.base.saturating_add(
            self.started
                .map_or(Duration::ZERO, |at| self.clock.now().saturating_sub(at)),
        )
    }
    pub fn snapshot(&self) -> Snapshot {
        let elapsed = match self.kind {
            Kind::Stopwatch => self.raw_elapsed(),
            Kind::Countdown(max) => self.raw_elapsed().min(max),
        };
        Snapshot {
            elapsed,
            remaining: match self.kind {
                Kind::Stopwatch => None,
                Kind::Countdown(max) => Some(max.saturating_sub(elapsed)),
            },
            running: self.running(),
            completed: self.completed,
        }
    }
    pub fn start(&mut self) -> bool {
        if self.started.is_some() || self.completed {
            return false;
        }
        self.started = Some(self.clock.now());
        self.last_bucket = None;
        true
    }
    pub fn pause(&mut self) -> bool {
        let Some(at) = self.started.take() else {
            return false;
        };
        self.base = self
            .base
            .saturating_add(self.clock.now().saturating_sub(at));
        self.last_bucket = None;
        true
    }
    pub fn reset(&mut self) {
        self.base = Duration::ZERO;
        self.started = None;
        self.completed = false;
        self.completion_pending = false;
        self.laps.clear();
        self.last_bucket = None;
    }
    pub fn lap(&mut self) -> bool {
        if self.started.is_none() || self.laps.len() >= MAX_LAPS {
            return false;
        }
        self.laps.push(self.snapshot().elapsed);
        true
    }
    pub fn tick(&mut self) -> Tick {
        if self.started.is_none() {
            return Tick {
                redraw: false,
                completed_now: false,
            };
        }
        let current = self.snapshot();
        let bucket = match self.kind {
            Kind::Stopwatch => current.elapsed.as_millis() / 100,
            Kind::Countdown(_) => current
                .remaining
                .unwrap_or(Duration::ZERO)
                .as_millis()
                .div_ceil(100),
        };
        let mut changed = self.last_bucket != Some(bucket);
        self.last_bucket = Some(bucket);
        let mut completed_now = false;
        if current.remaining == Some(Duration::ZERO) && !self.completed {
            self.completed = true;
            self.completion_pending = true;
            self.started = None;
            if let Kind::Countdown(max) = self.kind {
                self.base = max;
            }
            completed_now = true;
            changed = true;
        }
        Tick {
            redraw: changed,
            completed_now,
        }
    }
    /// Explicitly consumes the completion edge. Exactly one notification per
    /// countdown run, even if a slow dispatch misses many logical ticks.
    pub fn take_completion(&mut self) -> bool {
        std::mem::take(&mut self.completion_pending)
    }
}

pub fn formatted(value: Duration) -> String {
    let tenths = value.as_millis() / 100;
    let seconds = tenths / 10;
    format!("{:02}:{:02}.{}", seconds / 60, seconds % 60, tenths % 10)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{cell::Cell, rc::Rc};
    #[derive(Clone)]
    struct Fake(Rc<Cell<Duration>>);
    impl Fake {
        fn new() -> Self {
            Self(Rc::new(Cell::new(Duration::ZERO)))
        }
        fn advance(&self, d: Duration) {
            self.0.set(self.0.get().saturating_add(d));
        }
        fn set(&self, d: Duration) {
            self.0.set(d);
        }
    }
    impl Clock for Fake {
        fn now(&self) -> Duration {
            self.0.get()
        }
    }
    #[test]
    fn monotonic_stopwatch_pause_resume_lap_reset() {
        let clock = Fake::new();
        let mut t = Timer::new(clock.clone(), Kind::Stopwatch).unwrap();
        assert!(t.start());
        assert!(!t.start());
        clock.advance(Duration::from_millis(3200));
        assert_eq!(t.snapshot().elapsed, Duration::from_millis(3200));
        assert!(t.lap());
        assert_eq!(t.laps.len(), 1);
        assert!(t.pause());
        clock.advance(Duration::from_secs(30));
        assert_eq!(t.snapshot().elapsed, Duration::from_millis(3200));
        assert!(!t.tick().redraw);
        assert!(t.start());
        clock.advance(Duration::from_millis(800));
        t.pause();
        assert_eq!(t.snapshot().elapsed, Duration::from_secs(4));
        t.reset();
        assert_eq!(t.snapshot().elapsed, Duration::ZERO);
        assert!(t.laps.is_empty());
    }
    #[test]
    fn completion_notification_is_exactly_once() {
        let clock = Fake::new();
        let mut t = Timer::new(clock.clone(), Kind::Countdown(Duration::from_secs(2))).unwrap();
        assert!(t.start());
        clock.advance(Duration::from_millis(500));
        assert!(t.tick().redraw);
        assert!(!t.tick().redraw);
        clock.advance(Duration::from_secs(3));
        let end = t.tick();
        assert!(end.completed_now);
        assert!(!t.running());
        assert!(t.snapshot().completed);
        assert_eq!(t.snapshot().remaining, Some(Duration::ZERO));
        assert!(t.take_completion());
        assert!(!t.take_completion());
        assert!(!t.tick().redraw);
        assert!(!t.start());
        t.reset();
        assert!(t.start());
        clock.advance(Duration::from_secs(2));
        assert!(t.tick().completed_now);
        assert!(t.take_completion());
    }
    #[test]
    fn clock_rollback_never_subtracts_negative_time() {
        let clock = Fake::new();
        clock.set(Duration::from_secs(10));
        let mut t = Timer::new(clock.clone(), Kind::Stopwatch).unwrap();
        t.start();
        clock.set(Duration::from_secs(1));
        assert_eq!(t.snapshot().elapsed, Duration::ZERO);
        t.pause();
        assert_eq!(t.snapshot().elapsed, Duration::ZERO);
    }
    #[test]
    fn lap_and_duration_are_bounded() {
        assert!(Timer::new(Fake::new(), Kind::Countdown(Duration::ZERO)).is_err());
        assert!(Timer::new(
            Fake::new(),
            Kind::Countdown(MAX_COUNTDOWN + Duration::from_millis(1))
        )
        .is_err());
        let clock = Fake::new();
        let mut t = Timer::new(clock.clone(), Kind::Stopwatch).unwrap();
        t.start();
        for _ in 0..MAX_LAPS {
            assert!(t.lap());
        }
        assert!(!t.lap());
        assert_eq!(t.laps.len(), MAX_LAPS);
        assert_eq!(formatted(Duration::from_millis(63010)), "01:03.0");
    }
    #[test]
    fn paused_ticks_do_not_trigger_repaints() {
        let clock = Fake::new();
        let mut t = Timer::new(clock.clone(), Kind::Countdown(Duration::from_secs(90))).unwrap();
        assert!(!t.tick().redraw);
        t.start();
        assert!(t.tick().redraw);
        assert!(!t.tick().redraw);
        clock.advance(ACTIVE_TICK);
        assert!(t.tick().redraw);
        t.pause();
        clock.advance(Duration::from_secs(50));
        assert!(!t.tick().redraw);
    }
}
