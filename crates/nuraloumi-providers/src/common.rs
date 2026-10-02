use crate::error::ProviderError;
use std::time::{SystemTime, UNIX_EPOCH};

pub trait Provider {
    type Snapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError>;
}

pub trait ActionProvider<A> {
    fn execute(&mut self, action: A) -> Result<ActionResult, ProviderError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    Healthy,
    Degraded,
    Unavailable,
}

impl Health {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Unavailable => "unavailable",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotMeta {
    pub timestamp_ms: u64,
    pub health: Health,
    pub stale: bool,
    pub source: String,
}

impl SnapshotMeta {
    pub fn new(timestamp_ms: u64, health: Health, stale: bool, source: impl Into<String>) -> Self {
        Self {
            timestamp_ms,
            health,
            stale,
            source: source.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionResult {
    pub executed: bool,
    pub dry_run: bool,
    pub message: String,
}

impl ActionResult {
    pub fn executed(message: impl Into<String>) -> Self {
        Self {
            executed: true,
            dry_run: false,
            message: message.into(),
        }
    }

    pub fn dry_run(message: impl Into<String>) -> Self {
        Self {
            executed: false,
            dry_run: true,
            message: message.into(),
        }
    }
}

pub(crate) fn timestamp_ms(override_value: Option<u64>) -> u64 {
    override_value.unwrap_or_else(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    })
}
