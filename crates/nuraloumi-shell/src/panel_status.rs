//! Pure scheduling for the persistent panel's asynchronous status probes.
//!
//! A one-second fd poll is only a wakeup cadence. It must never itself run
//! subprocesses or read sysfs on the compositor/Wayland thread.
use nuraloumi_providers::{BatteryProvider, BatterySnapshot, Provider, ProviderError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Only cheap panel indicators refresh every 30s. A full ProbeSnapshot would
/// launch audio/network/Bluetooth commands repeatedly on the dual-core SL101.
pub struct PanelStatusSample {
    pub battery: Result<BatterySnapshot, ProviderError>,
    pub renderer: &'static str,
}

impl PanelStatusSample {
    pub fn live() -> Self {
        Self {
            battery: BatteryProvider::system().snapshot(),
            renderer: nuraloumi_providers::renderer::running_renderer(),
        }
    }
}

pub const FULL_REFRESH: Duration = Duration::from_secs(30);
pub const HUNG_FULL_REFRESH: Duration = Duration::from_secs(120);
pub const HUNG_CLOCK_REFRESH: Duration = Duration::from_secs(10);
const CLOCK_FAILURE_RETRY: Duration = Duration::from_secs(5);

/// Timestamp selection is wall-clock based so a resume or manual time jump
/// immediately invalidates a previously rendered HH:MM.
pub fn wall_minute(now: SystemTime) -> Option<u64> {
    now.duration_since(UNIX_EPOCH)
        .ok()
        .map(|age| age.as_secs() / 60)
}

#[derive(Debug)]
pub struct PanelStatusSchedule {
    last_full_start: Instant,
    last_clock_start: Option<Instant>,
    last_clock_minute: Option<u64>,
    clock_retry: bool,
    full_hang_reported: bool,
    clock_hang_reported: bool,
}

impl PanelStatusSchedule {
    pub fn new(now: Instant, wall: SystemTime) -> Self {
        Self {
            last_full_start: now,
            last_clock_start: None,
            last_clock_minute: wall_minute(wall),
            clock_retry: false,
            full_hang_reported: false,
            clock_hang_reported: false,
        }
    }

    /// Never start a second full snapshot while a prior worker remains alive.
    pub fn start_full(&mut self, now: Instant, busy: bool) -> bool {
        if busy || now.saturating_duration_since(self.last_full_start) < FULL_REFRESH {
            return false;
        }
        self.last_full_start = now;
        self.full_hang_reported = false;
        true
    }

    /// Launch a bounded date helper only at a minute transition, wall-clock
    /// jump, or a failed read's retry deadline; not at 1 Hz.
    pub fn start_clock(&mut self, now: Instant, wall: SystemTime, busy: bool) -> bool {
        if busy {
            return false;
        }
        let minute = wall_minute(wall);
        let changed = minute != self.last_clock_minute;
        let retry = self.clock_retry
            && self
                .last_clock_start
                .is_none_or(|at| now.saturating_duration_since(at) >= CLOCK_FAILURE_RETRY);
        if !changed && !retry {
            return false;
        }
        self.last_clock_start = Some(now);
        self.last_clock_minute = minute;
        self.clock_retry = false;
        self.clock_hang_reported = false;
        true
    }

    pub fn clock_finished(&mut self, successful: bool) {
        self.clock_retry = !successful;
        self.clock_hang_reported = false;
    }

    pub fn full_finished(&mut self) {
        self.full_hang_reported = false;
    }

    /// Mark a permanently stuck worker *once*, without leaking replacements.
    pub fn full_hung(&mut self, now: Instant, busy: bool) -> bool {
        if !busy
            || self.full_hang_reported
            || now.saturating_duration_since(self.last_full_start) < HUNG_FULL_REFRESH
        {
            return false;
        }
        self.full_hang_reported = true;
        true
    }

    pub fn clock_hung(&mut self, now: Instant, busy: bool) -> bool {
        if !busy
            || self.clock_hang_reported
            || self
                .last_clock_start
                .is_none_or(|at| now.saturating_duration_since(at) < HUNG_CLOCK_REFRESH)
        {
            return false;
        }
        self.clock_hang_reported = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minute_boundary_jump_and_resume_are_immediate_and_coalesced() {
        let t = Instant::now();
        let sec = |s| UNIX_EPOCH + Duration::from_secs(s);
        let mut s = PanelStatusSchedule::new(t, sec(119));
        assert!(!s.start_clock(t + Duration::from_secs(1), sec(119), false));
        assert!(s.start_clock(t + Duration::from_secs(1), sec(120), false));
        assert!(!s.start_clock(t + Duration::from_secs(2), sec(120), false));
        assert!(!s.start_clock(t + Duration::from_secs(2), sec(180), true));
        assert!(s.start_clock(t + Duration::from_secs(3), sec(180), false));
        assert!(s.start_clock(t + Duration::from_secs(4), sec(60), false));
        assert!(s.start_clock(t + Duration::from_secs(5), sec(3600), false));
    }

    #[test]
    fn full_poll_is_bounded_and_has_no_overlapping_workers() {
        let t = Instant::now();
        let mut s = PanelStatusSchedule::new(t, UNIX_EPOCH);
        assert!(!s.start_full(t + Duration::from_secs(29), false));
        assert!(!s.start_full(t + Duration::from_secs(30), true));
        assert!(s.start_full(t + Duration::from_secs(30), false));
        assert!(!s.start_full(t + Duration::from_secs(40), false));
        assert!(s.start_full(t + Duration::from_secs(60), false));
        assert!(!s.full_hung(t + Duration::from_secs(100), true));
        assert!(s.full_hung(t + Duration::from_secs(180), true));
        assert!(!s.full_hung(t + Duration::from_secs(181), true));
        s.full_finished();
        assert!(!s.full_hung(t + Duration::from_secs(181), false));
    }

    #[test]
    fn stuck_clock_does_not_spawn_more_workers_and_failure_retries() {
        let t = Instant::now();
        let mut s = PanelStatusSchedule::new(t, UNIX_EPOCH);
        let minute_one = UNIX_EPOCH + Duration::from_secs(60);
        assert!(s.start_clock(t, minute_one, false));
        assert!(!s.start_clock(t + Duration::from_secs(20), minute_one, true));
        assert!(s.clock_hung(t + Duration::from_secs(10), true));
        assert!(!s.clock_hung(t + Duration::from_secs(11), true));
        s.clock_finished(false);
        assert!(!s.start_clock(t + Duration::from_secs(4), minute_one, false));
        assert!(s.start_clock(t + Duration::from_secs(5), minute_one, false));
        s.clock_finished(true);
        assert!(!s.start_clock(t + Duration::from_secs(11), minute_one, false));
    }
}
