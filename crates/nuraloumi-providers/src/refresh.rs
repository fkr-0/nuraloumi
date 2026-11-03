//! Demand-driven, rate-limited snapshots with bounded stale/error recovery.
//!
//! This wrapper never starts polling or runs commands in render code. Callers
//! retain the provider between requests and schedule queries outside rendering.

use crate::audio::AudioSnapshot;
use crate::bluetooth::BluetoothSnapshot;
use crate::common::{ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use crate::network::NetworkSnapshot;
use std::time::{Duration, Instant};

const MIN_POLL_MS: u64 = 1_000;
const MAX_STALE_MS: u64 = 300_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshPolicy {
    poll_ms: u64,
    retry_ms: u64,
    stale_ms: u64,
}

impl RefreshPolicy {
    /// Poll and failure retry are never faster than one second.
    pub fn new(poll: Duration, retry: Duration, max_stale: Duration) -> Self {
        fn ms(value: Duration) -> u64 {
            value.as_millis().try_into().unwrap_or(u64::MAX)
        }
        Self {
            poll_ms: ms(poll).max(MIN_POLL_MS),
            retry_ms: ms(retry).max(MIN_POLL_MS),
            stale_ms: ms(max_stale).min(MAX_STALE_MS),
        }
    }

    pub fn poll_interval(self) -> Duration {
        Duration::from_millis(self.poll_ms)
    }

    pub fn retry_interval(self) -> Duration {
        Duration::from_millis(self.retry_ms)
    }

    pub fn max_stale(self) -> Duration {
        Duration::from_millis(self.stale_ms)
    }
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self::new(
            Duration::from_secs(5),
            Duration::from_secs(3),
            Duration::from_secs(60),
        )
    }
}

pub trait RefreshSnapshot: Clone {
    fn meta(&self) -> &SnapshotMeta;
    fn meta_mut(&mut self) -> &mut SnapshotMeta;
    fn issues(&self) -> &[String];
    fn issues_mut(&mut self) -> &mut Vec<String>;
}

macro_rules! refresh_snapshot {
    ($name:ty) => {
        impl RefreshSnapshot for $name {
            fn meta(&self) -> &SnapshotMeta {
                &self.meta
            }
            fn meta_mut(&mut self) -> &mut SnapshotMeta {
                &mut self.meta
            }
            fn issues(&self) -> &[String] {
                &self.issues
            }
            fn issues_mut(&mut self) -> &mut Vec<String> {
                &mut self.issues
            }
        }
    };
}
refresh_snapshot!(NetworkSnapshot);
refresh_snapshot!(AudioSnapshot);
refresh_snapshot!(BluetoothSnapshot);

/// Wrap a persistent NetworkProvider, AudioProvider, or BluetoothProvider.
///
/// Complete failures return the last healthy value *as degraded/stale* for
/// at most max_stale, then return the actual error/unavailable observation.
/// Degraded but usable live observations pass through without being hidden.
pub struct CachedProvider<P: Provider>
where
    P::Snapshot: RefreshSnapshot,
{
    provider: P,
    policy: RefreshPolicy,
    started: Instant,
    attempted_at: Option<u64>,
    succeeded_at: Option<u64>,
    healthy: Option<P::Snapshot>,
    observed: Option<Result<P::Snapshot, ProviderError>>,
    failed: bool,
}

impl<P: Provider> CachedProvider<P>
where
    P::Snapshot: RefreshSnapshot,
{
    pub fn new(provider: P, policy: RefreshPolicy) -> Self {
        Self {
            provider,
            policy,
            started: Instant::now(),
            attempted_at: None,
            succeeded_at: None,
            healthy: None,
            observed: None,
            failed: false,
        }
    }

    pub fn policy(&self) -> RefreshPolicy {
        self.policy
    }

    pub fn into_inner(self) -> P {
        self.provider
    }

    /// Supply a monotonic or Unix millisecond clock for deterministic fixtures.
    /// Consecutive calls with the same clock value never launch repeated probes.
    pub fn snapshot_at(&mut self, now_ms: u64) -> Result<P::Snapshot, ProviderError> {
        let cadence = if self.failed {
            self.policy.retry_ms
        } else {
            self.policy.poll_ms
        };
        if self
            .attempted_at
            .is_none_or(|at| now_ms.saturating_sub(at) >= cadence)
        {
            self.attempted_at = Some(now_ms);
            let result = self.provider.snapshot();
            match &result {
                Ok(value) if value.meta().health == Health::Healthy && !value.meta().stale => {
                    self.healthy = Some(value.clone());
                    self.succeeded_at = Some(now_ms);
                    self.failed = false;
                }
                Ok(value) => {
                    self.failed = value.meta().health == Health::Unavailable || value.meta().stale;
                }
                Err(_) => self.failed = true,
            }
            self.observed = Some(result);
        }

        let last = self
            .observed
            .as_ref()
            .expect("initial cached-provider call always probes");
        match last {
            Ok(snapshot) if snapshot.meta().health != Health::Unavailable => Ok(snapshot.clone()),
            Ok(snapshot) => self
                .stale_at(now_ms, &snapshot.issues().join("; "))
                .map(Ok)
                .unwrap_or_else(|| Ok(snapshot.clone())),
            Err(error) => self
                .stale_at(now_ms, &error.diagnostic())
                .map(Ok)
                .unwrap_or_else(|| Err(error.clone())),
        }
    }

    fn stale_at(&self, now_ms: u64, diagnostic: &str) -> Option<P::Snapshot> {
        let age = now_ms.saturating_sub(self.succeeded_at?);
        if age > self.policy.stale_ms {
            return None;
        }
        let mut value = self.healthy.clone()?;
        value.meta_mut().stale = true;
        value.meta_mut().health = Health::Degraded;
        // Leave timestamp_ms at the time of the actual healthy acquisition.
        value.issues_mut().push(format!(
            "last known good state ({age} ms old); refresh failed: {diagnostic}"
        ));
        Some(value)
    }

    /// Invalidate after suspend/resume or a service topology change.
    /// Successful mutating actions call this automatically.
    pub fn invalidate(&mut self) {
        self.attempted_at = None;
        self.succeeded_at = None;
        self.healthy = None;
        self.observed = None;
        self.failed = false;
    }
}

impl<P: Provider> Provider for CachedProvider<P>
where
    P::Snapshot: RefreshSnapshot,
{
    type Snapshot = P::Snapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let now_ms = self
            .started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX);
        self.snapshot_at(now_ms)
    }
}

impl<P, A> ActionProvider<A> for CachedProvider<P>
where
    P: Provider + ActionProvider<A>,
    P::Snapshot: RefreshSnapshot,
{
    fn execute(&mut self, action: A) -> Result<ActionResult, ProviderError> {
        let result = self.provider.execute(action);
        // Even a failed action may have partially changed system state.
        // Dry runs are explicit non-mutations and can retain the cache.
        if match &result {
            Ok(outcome) => outcome.executed,
            Err(_) => true,
        } {
            self.invalidate();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_and_retry_are_clamped_to_one_hz() {
        let policy = RefreshPolicy::new(Duration::ZERO, Duration::from_millis(10), Duration::ZERO);
        assert_eq!(policy.poll_interval(), Duration::from_secs(1));
        assert_eq!(policy.retry_interval(), Duration::from_secs(1));
        assert_eq!(policy.max_stale(), Duration::ZERO);
        let capped = RefreshPolicy::new(Duration::ZERO, Duration::ZERO, Duration::MAX);
        assert_eq!(capped.max_stale(), Duration::from_secs(300));
    }
}
