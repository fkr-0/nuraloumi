use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const SNAPSHOT_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(2), 256 * 1024);
const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(3), 32 * 1024);
/// Total wall-clock budget for an entire multi-ID clear; not one timeout per ID.
const CLEAR_BUDGET: Duration = Duration::from_secs(3);
const MAX_NOTIFICATIONS: usize = 64;
const MAX_ACTIONS: usize = 16;
const MAX_JSON_DEPTH: usize = 32;
const MAX_TEXT_CHARS: usize = 240;
const MAX_HISTORY_BYTES: usize = 256 * 1024;
const MAX_ISSUES: usize = 16;
const MIN_REFRESH_MS: u64 = 1_000;
const MAX_DISPLAY_TIMEOUT_MS: u64 = 120_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationEntry {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<String>,
    pub default_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationSnapshot {
    pub meta: SnapshotMeta,
    pub backend: String,
    pub notifications: Vec<NotificationEntry>,
    pub issues: Vec<String>,
}

impl NotificationSnapshot {
    pub fn unavailable(timestamp: u64, issue: String) -> Self {
        Self {
            meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, "dunstctl"),
            backend: "dunstctl".to_owned(),
            notifications: Vec::new(),
            issues: vec![issue],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationAction {
    Redisplay {
        id: u32,
    },
    Remove {
        id: u32,
    },
    Invoke {
        id: u32,
        action: String,
    },
    /// Legacy clear: restrict to the history entries observed at execution time.
    Clear,
    /// Only remove IDs captured by the UI at confirmation time.
    ClearKnown {
        ids: Vec<u32>,
    },
}

pub struct NotificationProvider<R> {
    runner: R,
    timestamp_override: Option<u64>,
    unsafe_best_effort_actions: bool,
}

impl NotificationProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self::new(SystemCommandRunner)
    }
}

impl<R> NotificationProvider<R> {
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            timestamp_override: None,
            unsafe_best_effort_actions: false,
        }
    }

    /// Opt in to Dunst's non-atomic history-pop -> action sequence.
    /// It can target a different notification if another sender races.
    /// Safe production use MUST leave this disabled.
    pub fn with_unsafe_best_effort_actions(mut self, enabled: bool) -> Self {
        self.unsafe_best_effort_actions = enabled;
        self
    }

    pub fn with_timestamp(mut self, timestamp: Option<u64>) -> Self {
        self.timestamp_override = timestamp;
        self
    }

    pub fn into_runner(self) -> R {
        self.runner
    }
}

impl<R: CommandRunner> Provider for NotificationProvider<R> {
    type Snapshot = NotificationSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let output = match self.runner.run(
            &CommandSpec::new("dunstctl").arg("history"),
            SNAPSHOT_LIMITS,
        ) {
            Ok(output) if output.status == 0 => output,
            Ok(output) => {
                return Ok(NotificationSnapshot::unavailable(
                    timestamp,
                    format!("dunstctl history exited with status {}", output.status),
                ))
            }
            Err(error) => {
                return Ok(NotificationSnapshot::unavailable(
                    timestamp,
                    error.diagnostic(),
                ))
            }
        };

        match parse_history(&output.stdout) {
            Ok((notifications, issues)) => {
                let health = if issues.is_empty() {
                    Health::Healthy
                } else {
                    Health::Degraded
                };
                Ok(NotificationSnapshot {
                    meta: SnapshotMeta::new(timestamp, health, false, "dunstctl"),
                    backend: "dunstctl".to_owned(),
                    notifications,
                    issues,
                })
            }
            Err(error) => Ok(NotificationSnapshot::unavailable(
                timestamp,
                error.diagnostic(),
            )),
        }
    }
}

impl<R: CommandRunner> ActionProvider<NotificationAction> for NotificationProvider<R> {
    fn execute(&mut self, action: NotificationAction) -> Result<ActionResult, ProviderError> {
        let snapshot = self.snapshot()?;
        match action {
            NotificationAction::Clear => {
                // Legacy callers cannot freeze the UI confirmation set.
                // At minimum, never call the global history-clear command:
                // remove only the IDs seen by this snapshot.
                let ids = snapshot
                    .notifications
                    .iter()
                    .map(|entry| entry.id)
                    .collect::<Vec<_>>();
                clear_exact_history(&mut self.runner, &snapshot, &ids)
            }
            NotificationAction::ClearKnown { ids } => {
                // Caller must capture IDs when the user is asked to confirm.
                // New arrivals cannot be added by the later history read.
                clear_exact_history(&mut self.runner, &snapshot, &ids)
            }
            NotificationAction::Redisplay { id } => {
                require_notification(&snapshot, id)?;
                run_action(&mut self.runner, ["history-pop".to_owned(), id.to_string()])?;
                Ok(ActionResult::executed(format!(
                    "notification {id} redisplayed"
                )))
            }
            NotificationAction::Remove { id } => {
                require_notification(&snapshot, id)?;
                run_action(&mut self.runner, ["history-rm".to_owned(), id.to_string()])?;
                Ok(ActionResult::executed(format!(
                    "notification {id} removed from history"
                )))
            }
            NotificationAction::Invoke { id, action } => {
                let entry = require_notification(&snapshot, id)?;
                validate_action_name(&action)?;
                if !entry.actions.iter().any(|candidate| candidate == &action)
                    && entry.default_action.as_deref() != Some(action.as_str())
                {
                    return Err(ProviderError::unavailable(format!(
                        "notification {id} does not expose action {action:?}"
                    )));
                }

                if !self.unsafe_best_effort_actions {
                    return Err(ProviderError::unavailable(
                        "Dunst cannot atomically invoke actions by history ID; unsafe best-effort invocation is disabled",
                    ));
                }
                // This explicitly opted-in path remains non-atomic! Dunst
                // addresses actions by displayed position, not history ID.
                // Redisplay the exact history entry first, then
                // invoke its validated action as the new topmost notification.
                run_action(&mut self.runner, ["history-pop".to_owned(), id.to_string()])?;
                run_action(
                    &mut self.runner,
                    ["action".to_owned(), "0".to_owned(), action.clone()],
                )?;
                Ok(ActionResult::executed(format!(
                    "notification {id} action {action:?} invoked"
                )))
            }
        }
    }
}

fn run_action<R: CommandRunner, I>(runner: &mut R, args: I) -> Result<(), ProviderError>
where
    I: IntoIterator<Item = String>,
{
    run_action_limited(runner, args, ACTION_LIMITS)
}

fn run_action_limited<R: CommandRunner, I>(
    runner: &mut R,
    args: I,
    limits: CommandLimits,
) -> Result<(), ProviderError>
where
    I: IntoIterator<Item = String>,
{
    let output = runner.run(&CommandSpec::new("dunstctl").args(args), limits)?;
    if output.status != 0 {
        return Err(ProviderError::backend(format!(
            "dunstctl action exited with status {}",
            output.status
        )));
    }
    Ok(())
}

fn clear_exact_history<R: CommandRunner>(
    runner: &mut R,
    snapshot: &NotificationSnapshot,
    ids: &[u32],
) -> Result<ActionResult, ProviderError> {
    clear_exact_history_with_budget(runner, snapshot, ids, CLEAR_BUDGET)
}

fn clear_exact_history_with_budget<R: CommandRunner>(
    runner: &mut R,
    snapshot: &NotificationSnapshot,
    ids: &[u32],
    budget: Duration,
) -> Result<ActionResult, ProviderError> {
    if snapshot.meta.health != Health::Healthy {
        return Err(ProviderError::unavailable(
            "notification history is not healthy; clear cannot be validated",
        ));
    }
    if ids.len() > MAX_NOTIFICATIONS {
        return Err(ProviderError::parse("too many notification IDs to clear"));
    }
    let mut seen = BTreeSet::new();
    for id in ids {
        if !seen.insert(*id) {
            return Err(ProviderError::parse(
                "duplicate notification ID in clear request",
            ));
        }
        require_notification(snapshot, *id)?;
    }
    // Per-command timeouts MUST share one deadline, otherwise 64 entries
    // could block for 64 * 3 seconds. The caller runs this off the UI thread.
    // Dunst does not offer an atomic specific-ID batch operation. If a command
    // fails or the global budget runs out, report partial completion.
    let started = Instant::now();
    for id in ids {
        let remaining = budget.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(ProviderError::timeout(
                "notification clear deadline exhausted; history may be partially cleared",
            ));
        }
        run_action_limited(
            runner,
            ["history-rm".to_owned(), id.to_string()],
            CommandLimits::new(
                remaining.min(ACTION_LIMITS.timeout),
                ACTION_LIMITS.max_output_bytes,
            ),
        )?;
    }
    Ok(ActionResult::executed(format!(
        "removed {} known notification(s) from history",
        ids.len()
    )))
}

fn require_notification(
    snapshot: &NotificationSnapshot,
    id: u32,
) -> Result<&NotificationEntry, ProviderError> {
    snapshot
        .notifications
        .iter()
        .find(|notification| notification.id == id)
        .ok_or_else(|| {
            ProviderError::unavailable(format!("notification {id} is not in current history"))
        })
}

fn validate_action_name(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
    {
        return Err(ProviderError::parse("invalid notification action name"));
    }
    Ok(())
}

fn parse_history(text: &str) -> Result<(Vec<NotificationEntry>, Vec<String>), ProviderError> {
    let (records, issues) = parse_history_records(text)?;
    Ok((
        records.into_iter().map(|record| record.entry).collect(),
        issues,
    ))
}

fn parse_history_records(text: &str) -> Result<(Vec<ParsedRecord>, Vec<String>), ProviderError> {
    if text.len() > MAX_HISTORY_BYTES {
        return Err(ProviderError::parse("dunst history exceeds byte limit"));
    }
    let root = JsonParser::new(text).parse()?;
    let entries = root
        .object()
        .and_then(|object| object.get("data"))
        .and_then(JsonValue::array)
        .ok_or_else(|| ProviderError::parse("dunst history root has no data array"))?;

    let mut notifications = Vec::new();
    let mut issues = Vec::new();
    collect_history_entries(entries, &mut notifications, &mut issues);
    Ok((notifications, issues))
}

fn collect_history_entries(
    entries: &[JsonValue],
    notifications: &mut Vec<ParsedRecord>,
    issues: &mut Vec<String>,
) {
    for entry in entries {
        if notifications.len() >= MAX_NOTIFICATIONS {
            return;
        }
        if let Some(group) = entry.array() {
            collect_history_entries(group, notifications, issues);
            continue;
        }

        let Some(object) = entry.object() else {
            push_issue(issues, "dunst history contained a non-object entry");
            continue;
        };
        let Some(id) = variant_i64(object.get("id")).and_then(|id| u32::try_from(id).ok()) else {
            push_issue(issues, "dunst history entry missing valid id");
            continue;
        };
        if notifications.iter().any(|record| record.entry.id == id) {
            push_issue(issues, "dunst history contained a duplicate id");
            continue;
        }

        let app = bounded_text(&variant_string(object.get("appname")).unwrap_or_default());
        let summary = bounded_text(&variant_string(object.get("summary")).unwrap_or_default());
        let body = bounded_text(&variant_string(object.get("body")).unwrap_or_default());
        let default_action = variant_string(object.get("default_action_name"))
            .filter(|value| validate_action_name(value).is_ok());
        let mut actions = variant_action_names(object.get("actions"));
        if let Some(default_action) = &default_action {
            if !actions.iter().any(|action| action == default_action) {
                actions.push(default_action.clone());
            }
        }
        actions.truncate(MAX_ACTIONS);

        let urgency = match variant_string(object.get("urgency"))
            .unwrap_or_default()
            .to_ascii_uppercase()
            .as_str()
        {
            "LOW" => NotificationUrgency::Low,
            "NORMAL" => NotificationUrgency::Normal,
            "CRITICAL" => NotificationUrgency::Critical,
            _ => NotificationUrgency::Unknown,
        };
        // Dunst represents display timeout in microseconds; not history TTL.
        let timeout_ms = variant_i64(object.get("timeout"))
            .and_then(|v| u64::try_from(v).ok())
            .filter(|v| *v > 0)
            .map(|v| v.div_ceil(1000).min(MAX_DISPLAY_TIMEOUT_MS));
        notifications.push(ParsedRecord {
            entry: NotificationEntry {
                id,
                app,
                summary,
                body,
                actions,
                default_action,
            },
            urgency,
            timeout_ms,
        });
    }
}

fn variant_data(value: Option<&JsonValue>) -> Option<&JsonValue> {
    value?.object()?.get("data")
}

fn variant_string(value: Option<&JsonValue>) -> Option<String> {
    variant_data(value)?.string().map(ToOwned::to_owned)
}

fn variant_i64(value: Option<&JsonValue>) -> Option<i64> {
    variant_data(value)?.number()
}

fn variant_action_names(value: Option<&JsonValue>) -> Vec<String> {
    let Some(values) = variant_data(value).and_then(JsonValue::array) else {
        return Vec::new();
    };
    let raw = values
        .iter()
        .filter_map(JsonValue::string)
        .collect::<Vec<_>>();

    // Preserve the original key/label pairing before validation: labels may
    // contain arbitrary punctuation, whitespace or markup.
    let keys = if raw.len() >= 2 && raw.len() % 2 == 0 {
        raw.into_iter().step_by(2).collect::<Vec<_>>()
    } else {
        raw
    };
    keys.into_iter()
        .filter(|value| validate_action_name(value).is_ok())
        .take(MAX_ACTIONS)
        .map(ToOwned::to_owned)
        .collect()
}

fn bounded_text(value: &str) -> String {
    // Dunst history can contain remote untrusted HTML and terminal controls.
    // Remove tags before entity decoding; render only plain text.
    let chars: Vec<char> = value.chars().take(4096).collect();
    let mut plain = String::new();
    let mut index = 0;
    while index < chars.len() && plain.chars().count() <= MAX_TEXT_CHARS {
        let ch = chars[index];
        if ch == '<'
            && matches!(
                chars.get(index + 1),
                Some('a'..='z' | 'A'..='Z' | '/' | '!')
            )
        {
            if let Some(close) = chars[index + 1..].iter().take(128).position(|c| *c == '>') {
                index += close + 2;
                continue;
            }
        }
        if ch == '\u{1b}' {
            index += 1;
            if chars.get(index) == Some(&'[') {
                index += 1;
                while index < chars.len() {
                    let byte = chars[index];
                    index += 1;
                    if ('@'..='~').contains(&byte) {
                        break;
                    }
                }
            }
            continue;
        }
        if !(matches!(ch, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            || (ch.is_control() && ch != '\n' && ch != '\t'))
        {
            plain.push(if ch == '\t' { ' ' } else { ch });
        }
        index += 1;
    }
    let plain = plain
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    if plain.chars().count() <= MAX_TEXT_CHARS {
        return plain;
    }
    let mut result: String = plain.chars().take(MAX_TEXT_CHARS - 1).collect();
    result.push('…');
    result
}

fn push_issue(issues: &mut Vec<String>, message: &str) {
    if issues.len() < MAX_ISSUES {
        issues.push(message.to_owned());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotificationUrgency {
    Low,
    Normal,
    Critical,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedRecord {
    entry: NotificationEntry,
    urgency: NotificationUrgency,
    timeout_ms: Option<u64>,
}

/// The display timeout is metadata, not a history-retention deadline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveredNotification {
    pub entry: NotificationEntry,
    pub urgency: NotificationUrgency,
    pub unread: bool,
    pub first_observed_ms: u64,
    pub display_timeout_ms: Option<u64>,
    pub display_expired: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotificationDeliveryEvent {
    Added { id: u32 },
    Updated { id: u32 },
    Removed { id: u32 },
    DisplayExpired { id: u32 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationDeliverySnapshot {
    pub health: Health,
    pub notifications: Vec<DeliveredNotification>,
    pub unread_count: usize,
    pub events: Vec<NotificationDeliveryEvent>,
    pub issues: Vec<String>,
}

/// Passive, bounded Dunst history observer. Does NOT own the
/// org.freedesktop.Notifications DBus name, start a daemon or create threads.
/// Dunst history is populated after display/close, not on every Notify call.
pub struct NotificationDelivery<R> {
    provider: NotificationProvider<R>,
    started: Instant,
    entries: BTreeMap<u32, DeliveredNotification>,
    order: Vec<u32>,
    initialized: bool,
    last_poll_ms: Option<u64>,
    health: Health,
    issues: Vec<String>,
}

impl NotificationDelivery<SystemCommandRunner> {
    pub fn system() -> Self {
        Self::new(SystemCommandRunner)
    }
}

impl<R> NotificationDelivery<R> {
    pub fn new(runner: R) -> Self {
        Self {
            provider: NotificationProvider::new(runner),
            started: Instant::now(),
            entries: BTreeMap::new(),
            order: Vec::new(),
            initialized: false,
            last_poll_ms: None,
            health: Health::Unavailable,
            issues: Vec::new(),
        }
    }

    pub fn mark_read(&mut self, id: u32) -> bool {
        if let Some(entry) = self.entries.get_mut(&id) {
            if entry.unread {
                entry.unread = false;
                return true;
            }
        }
        false
    }

    pub fn mark_all_read(&mut self) {
        for entry in self.entries.values_mut() {
            entry.unread = false;
        }
    }

    pub fn snapshot(&self) -> NotificationDeliverySnapshot {
        self.snapshot_with_events(Vec::new())
    }

    fn snapshot_with_events(
        &self,
        events: Vec<NotificationDeliveryEvent>,
    ) -> NotificationDeliverySnapshot {
        let notifications: Vec<_> = self
            .order
            .iter()
            .filter_map(|id| self.entries.get(id).cloned())
            .collect();
        NotificationDeliverySnapshot {
            health: self.health,
            unread_count: notifications.iter().filter(|entry| entry.unread).count(),
            notifications,
            events,
            issues: self.issues.clone(),
        }
    }

    fn expire_display(&mut self, now_ms: u64) -> Vec<NotificationDeliveryEvent> {
        let mut events = Vec::new();
        for entry in self.entries.values_mut() {
            if !entry.display_expired
                && entry.display_timeout_ms.is_some_and(|timeout| {
                    now_ms >= entry.first_observed_ms.saturating_add(timeout)
                })
            {
                entry.display_expired = true;
                events.push(NotificationDeliveryEvent::DisplayExpired { id: entry.entry.id });
            }
        }
        events
    }

    fn reconcile(
        &mut self,
        records: Vec<ParsedRecord>,
        now_ms: u64,
        issues: Vec<String>,
    ) -> NotificationDeliverySnapshot {
        let mut events = Vec::new();
        let mut next = BTreeMap::new();
        let mut order = Vec::new();
        for record in records.into_iter().take(MAX_NOTIFICATIONS) {
            let id = record.entry.id;
            let old = self.entries.remove(&id);
            let entry = match old {
                Some(previous)
                    if previous.entry == record.entry
                        && previous.urgency == record.urgency
                        && previous.display_timeout_ms == record.timeout_ms =>
                {
                    previous
                }
                Some(_) => {
                    events.push(NotificationDeliveryEvent::Updated { id });
                    DeliveredNotification {
                        entry: record.entry,
                        urgency: record.urgency,
                        unread: true,
                        first_observed_ms: now_ms,
                        display_timeout_ms: record.timeout_ms,
                        display_expired: false,
                    }
                }
                None => {
                    if self.initialized {
                        events.push(NotificationDeliveryEvent::Added { id });
                    }
                    DeliveredNotification {
                        entry: record.entry,
                        urgency: record.urgency,
                        unread: self.initialized,
                        first_observed_ms: now_ms,
                        display_timeout_ms: record.timeout_ms,
                        display_expired: !self.initialized,
                    }
                }
            };
            next.insert(id, entry);
            order.push(id);
        }
        if self.initialized {
            for id in self.entries.keys() {
                events.push(NotificationDeliveryEvent::Removed { id: *id });
            }
        }
        self.entries = next;
        self.order = order;
        self.initialized = true;
        self.health = if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        self.issues = issues;
        events.extend(self.expire_display(now_ms));
        self.snapshot_with_events(events)
    }
}

impl<R: CommandRunner> NotificationDelivery<R> {
    /// Reads the live Dunst history without owning its DBus service.
    /// Actual monotonic time is internal, so callers cannot accidentally
    /// defeat the <= 1 Hz subprocess budget.
    pub fn poll(&mut self) -> NotificationDeliverySnapshot {
        let elapsed = self.started.elapsed().as_millis();
        let monotonic_ms = u64::try_from(elapsed).unwrap_or(u64::MAX);
        self.poll_at(monotonic_ms)
    }

    /// Injected clock is private to deterministic tests.
    fn poll_at(&mut self, monotonic_ms: u64) -> NotificationDeliverySnapshot {
        let mut expiry_events = self.expire_display(monotonic_ms);
        if let Some(last) = self.last_poll_ms {
            if monotonic_ms >= last && monotonic_ms - last < MIN_REFRESH_MS {
                return self.snapshot_with_events(expiry_events);
            }
        }
        self.last_poll_ms = Some(monotonic_ms);
        let text = match self.provider.runner.run(
            &CommandSpec::new("dunstctl").arg("history"),
            SNAPSHOT_LIMITS,
        ) {
            Ok(result) if result.status == 0 => result.stdout,
            Ok(result) => {
                self.health = Health::Unavailable;
                self.issues = vec![format!(
                    "dunstctl history exited with status {}",
                    result.status
                )];
                return self.snapshot_with_events(expiry_events);
            }
            Err(error) => {
                self.health = Health::Unavailable;
                self.issues = vec![error.diagnostic()];
                return self.snapshot_with_events(expiry_events);
            }
        };
        match parse_history_records(&text) {
            Ok((records, issues)) => {
                let mut snapshot = self.reconcile(records, monotonic_ms, issues);
                expiry_events.append(&mut snapshot.events);
                snapshot.events = expiry_events;
                snapshot
            }
            Err(error) => {
                self.health = Health::Unavailable;
                self.issues = vec![error.diagnostic()];
                self.snapshot_with_events(expiry_events)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum JsonValue {
    Null,
    Bool,
    Number(i64),
    String(String),
    Array(Vec<JsonValue>),
    Object(BTreeMap<String, JsonValue>),
}

impl JsonValue {
    pub(crate) fn object(&self) -> Option<&BTreeMap<String, JsonValue>> {
        match self {
            Self::Object(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn array(&self) -> Option<&[JsonValue]> {
        match self {
            Self::Array(value) => Some(value),
            _ => None,
        }
    }

    pub(crate) fn string(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            _ => None,
        }
    }

    fn number(&self) -> Option<i64> {
        match self {
            Self::Number(value) => Some(*value),
            _ => None,
        }
    }
}

pub(crate) struct JsonParser<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> JsonParser<'a> {
    pub(crate) fn new(input: &'a str) -> Self {
        Self {
            input: input.as_bytes(),
            offset: 0,
        }
    }

    pub(crate) fn parse(mut self) -> Result<JsonValue, ProviderError> {
        let value = self.parse_value(0)?;
        self.skip_ws();
        if self.offset != self.input.len() {
            return Err(ProviderError::parse("trailing data in dunst history JSON"));
        }
        Ok(value)
    }

    fn parse_value(&mut self, depth: usize) -> Result<JsonValue, ProviderError> {
        if depth > MAX_JSON_DEPTH {
            return Err(ProviderError::parse(
                "dunst history JSON exceeds depth limit",
            ));
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(depth + 1),
            Some(b'[') => self.parse_array(depth + 1),
            Some(b'"') => self.parse_string().map(JsonValue::String),
            Some(b'-' | b'0'..=b'9') => self.parse_number().map(JsonValue::Number),
            Some(b't') => {
                self.expect_literal(b"true")?;
                Ok(JsonValue::Bool)
            }
            Some(b'f') => {
                self.expect_literal(b"false")?;
                Ok(JsonValue::Bool)
            }
            Some(b'n') => {
                self.expect_literal(b"null")?;
                Ok(JsonValue::Null)
            }
            _ => Err(ProviderError::parse("invalid dunst history JSON value")),
        }
    }

    fn parse_object(&mut self, depth: usize) -> Result<JsonValue, ProviderError> {
        self.expect(b'{')?;
        self.skip_ws();
        let mut object = BTreeMap::new();
        if self.consume(b'}') {
            return Ok(JsonValue::Object(object));
        }
        loop {
            self.skip_ws();
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':')?;
            let value = self.parse_value(depth)?;
            object.insert(key, value);
            self.skip_ws();
            if self.consume(b'}') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Object(object))
    }

    fn parse_array(&mut self, depth: usize) -> Result<JsonValue, ProviderError> {
        self.expect(b'[')?;
        self.skip_ws();
        let mut values = Vec::new();
        if self.consume(b']') {
            return Ok(JsonValue::Array(values));
        }
        loop {
            values.push(self.parse_value(depth)?);
            self.skip_ws();
            if self.consume(b']') {
                break;
            }
            self.expect(b',')?;
        }
        Ok(JsonValue::Array(values))
    }

    fn parse_string(&mut self) -> Result<String, ProviderError> {
        self.expect(b'"')?;
        let mut output = String::new();
        loop {
            let byte = self
                .next()
                .ok_or_else(|| ProviderError::parse("unterminated JSON string"))?;
            match byte {
                b'"' => return Ok(output),
                b'\\' => {
                    let escaped = self
                        .next()
                        .ok_or_else(|| ProviderError::parse("unterminated JSON escape"))?;
                    match escaped {
                        b'"' => output.push('"'),
                        b'\\' => output.push('\\'),
                        b'/' => output.push('/'),
                        b'b' => output.push('\u{0008}'),
                        b'f' => output.push('\u{000c}'),
                        b'n' => output.push('\n'),
                        b'r' => output.push('\r'),
                        b't' => output.push('\t'),
                        b'u' => self.push_unicode_escape(&mut output)?,
                        _ => return Err(ProviderError::parse("invalid JSON escape")),
                    }
                }
                0x00..=0x1f => return Err(ProviderError::parse("control byte in JSON string")),
                first if first < 0x80 => output.push(first as char),
                first => {
                    let width = utf8_width(first)
                        .ok_or_else(|| ProviderError::parse("invalid UTF-8 in JSON string"))?;
                    let start = self.offset - 1;
                    let end = start.saturating_add(width);
                    if end > self.input.len() {
                        return Err(ProviderError::parse("truncated UTF-8 in JSON string"));
                    }
                    let slice = std::str::from_utf8(&self.input[start..end])
                        .map_err(|_| ProviderError::parse("invalid UTF-8 in JSON string"))?;
                    output.push_str(slice);
                    self.offset = end;
                }
            }
        }
    }

    fn push_unicode_escape(&mut self, output: &mut String) -> Result<(), ProviderError> {
        let first = self.parse_hex_u16()?;
        let scalar = if (0xd800..=0xdbff).contains(&first) {
            self.expect(b'\\')?;
            self.expect(b'u')?;
            let second = self.parse_hex_u16()?;
            if !(0xdc00..=0xdfff).contains(&second) {
                return Err(ProviderError::parse("invalid JSON surrogate pair"));
            }
            0x10000 + (((u32::from(first) - 0xd800) << 10) | (u32::from(second) - 0xdc00))
        } else if (0xdc00..=0xdfff).contains(&first) {
            return Err(ProviderError::parse("unexpected low JSON surrogate"));
        } else {
            u32::from(first)
        };
        let ch = char::from_u32(scalar)
            .ok_or_else(|| ProviderError::parse("invalid JSON unicode scalar"))?;
        output.push(ch);
        Ok(())
    }

    fn parse_hex_u16(&mut self) -> Result<u16, ProviderError> {
        let mut value = 0u16;
        for _ in 0..4 {
            let byte = self
                .next()
                .ok_or_else(|| ProviderError::parse("truncated JSON unicode escape"))?;
            let digit = match byte {
                b'0'..=b'9' => u16::from(byte - b'0'),
                b'a'..=b'f' => u16::from(byte - b'a' + 10),
                b'A'..=b'F' => u16::from(byte - b'A' + 10),
                _ => return Err(ProviderError::parse("invalid JSON unicode escape")),
            };
            value = (value << 4) | digit;
        }
        Ok(value)
    }

    fn parse_number(&mut self) -> Result<i64, ProviderError> {
        let start = self.offset;
        if self.peek() == Some(b'-') {
            self.offset += 1;
        }
        let digit_start = self.offset;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.offset += 1;
        }
        if self.offset == digit_start {
            return Err(ProviderError::parse("invalid JSON number"));
        }
        if matches!(self.peek(), Some(b'.' | b'e' | b'E')) {
            return Err(ProviderError::parse(
                "floating-point values are unsupported in dunst history",
            ));
        }
        std::str::from_utf8(&self.input[start..self.offset])
            .map_err(|_| ProviderError::parse("invalid JSON number encoding"))?
            .parse::<i64>()
            .map_err(|_| ProviderError::parse("JSON integer out of range"))
    }

    fn expect_literal(&mut self, literal: &[u8]) -> Result<(), ProviderError> {
        if self.input.get(self.offset..self.offset + literal.len()) == Some(literal) {
            self.offset += literal.len();
            Ok(())
        } else {
            Err(ProviderError::parse("invalid JSON literal"))
        }
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\n' | b'\r' | b'\t')) {
            self.offset += 1;
        }
    }

    fn expect(&mut self, byte: u8) -> Result<(), ProviderError> {
        if self.consume(byte) {
            Ok(())
        } else {
            Err(ProviderError::parse(format!(
                "expected JSON byte {:?}",
                byte as char
            )))
        }
    }

    fn consume(&mut self, byte: u8) -> bool {
        if self.peek() == Some(byte) {
            self.offset += 1;
            true
        } else {
            false
        }
    }

    fn peek(&self) -> Option<u8> {
        self.input.get(self.offset).copied()
    }

    fn next(&mut self) -> Option<u8> {
        let value = self.peek()?;
        self.offset += 1;
        Some(value)
    }
}

fn utf8_width(first: u8) -> Option<usize> {
    match first {
        0xc2..=0xdf => Some(2),
        0xe0..=0xef => Some(3),
        0xf0..=0xf4 => Some(4),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::{CommandOutput, FixtureCommandRunner};

    fn output(stdout: &str) -> CommandOutput {
        CommandOutput {
            status: 0,
            stdout: stdout.to_owned(),
            stderr: String::new(),
        }
    }

    fn history() -> &'static str {
        r#"{
          "type":"aa{sv}",
          "data":[[{
            "appname":{"type":"s","data":"Chat"},
            "summary":{"type":"s","data":"Message \ud83d\udcac"},
            "body":{"type":"s","data":"Hello\nworld"},
            "id":{"type":"i","data":42},
            "default_action_name":{"type":"s","data":"default"},
            "actions":{"type":"as","data":["default","Open","reply","Reply"]}
          }]]
        }"#
    }

    #[test]
    fn missing_dunstctl_is_unavailable_not_fatal() {
        let snapshot = NotificationProvider::new(FixtureCommandRunner::default())
            .with_timestamp(Some(3))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.health, Health::Unavailable);
        assert!(snapshot.notifications.is_empty());
    }

    #[test]
    fn parses_bounded_history_and_exact_remove_action() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("dunstctl").arg("history"),
            output(history()),
        );
        runner.insert(
            CommandSpec::new("dunstctl").args(["history-rm", "42"]),
            output(""),
        );

        let mut provider = NotificationProvider::new(runner).with_timestamp(Some(4));
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.notifications.len(), 1);
        assert_eq!(snapshot.notifications[0].summary, "Message 💬");
        assert_eq!(snapshot.notifications[0].body, "Hello\nworld");
        assert_eq!(
            snapshot.notifications[0].actions,
            vec!["default".to_owned(), "reply".to_owned()]
        );
        assert!(
            provider
                .execute(NotificationAction::Remove { id: 42 })
                .unwrap()
                .executed
        );
    }

    #[test]
    fn invoke_validates_history_id_and_action_before_two_step_dunst_call() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("dunstctl").arg("history"),
            output(history()),
        );
        runner.insert(
            CommandSpec::new("dunstctl").args(["history-pop", "42"]),
            output(""),
        );
        runner.insert(
            CommandSpec::new("dunstctl").args(["action", "0", "reply"]),
            output(""),
        );

        let result = NotificationProvider::new(runner)
            .with_unsafe_best_effort_actions(true)
            .execute(NotificationAction::Invoke {
                id: 42,
                action: "reply".to_owned(),
            })
            .unwrap();
        assert!(result.executed);
    }

    #[test]
    fn forged_action_fails_closed_before_mutation() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("dunstctl").arg("history"),
            output(history()),
        );

        let error = NotificationProvider::new(runner)
            .execute(NotificationAction::Invoke {
                id: 42,
                action: "reply;touch /tmp/nope".to_owned(),
            })
            .unwrap_err();
        assert_eq!(error.category, crate::ProviderErrorCategory::Parse);
    }

    #[derive(Clone)]
    struct SequenceRunner {
        outputs: std::collections::VecDeque<CommandOutput>,
        calls: std::rc::Rc<std::cell::Cell<usize>>,
    }

    impl SequenceRunner {
        fn with_outputs(outputs: Vec<CommandOutput>) -> Self {
            Self {
                outputs: outputs.into(),
                calls: std::rc::Rc::new(std::cell::Cell::new(0)),
            }
        }
    }

    impl CommandRunner for SequenceRunner {
        fn run(
            &mut self,
            spec: &CommandSpec,
            limits: CommandLimits,
        ) -> Result<CommandOutput, ProviderError> {
            assert_eq!(spec, &CommandSpec::new("dunstctl").arg("history"));
            assert_eq!(limits, SNAPSHOT_LIMITS);
            self.calls.set(self.calls.get() + 1);
            self.outputs
                .pop_front()
                .ok_or_else(|| ProviderError::unavailable("fixture depleted"))
        }
    }

    #[test]
    fn delivery_observes_real_backend_snapshots_and_tracks_unread_delta() {
        let empty = output(r#"{"data":[[]]}"#);
        let runner = SequenceRunner::with_outputs(vec![
            empty.clone(),
            output(history()),
            output(history()),
            empty,
        ]);
        let calls = runner.calls.clone();
        let mut feed = NotificationDelivery::new(runner);
        assert!(feed.poll_at(0).notifications.is_empty());
        assert!(feed.poll_at(300).events.is_empty());
        assert_eq!(calls.get(), 1, "refresh throttled to one call per second");

        let next = feed.poll_at(1_000);
        assert_eq!(
            next.events,
            vec![NotificationDeliveryEvent::Added { id: 42 }]
        );
        assert_eq!(next.unread_count, 1);
        assert_eq!(next.notifications[0].entry.summary, "Message 💬");
        assert!(feed.mark_read(42));
        assert!(!feed.mark_read(42));
        assert_eq!(feed.snapshot().unread_count, 0);
        assert!(feed.poll_at(2_000).events.is_empty());

        let removed = feed.poll_at(3_000);
        assert_eq!(
            removed.events,
            vec![NotificationDeliveryEvent::Removed { id: 42 }]
        );
        assert!(removed.notifications.is_empty());
        assert_eq!(calls.get(), 4);
    }

    #[test]
    fn baseline_is_read_and_display_timeout_does_not_erase_history() {
        let decorated = history()
            .replace(
                r#""id":{"type":"i","data":42},"#,
                r#""id":{"type":"i","data":42},"urgency":{"data":"CRITICAL"},"timeout":{"data":1500000},"#,
            )
            .replace(
                r#"Hello\nworld"#,
                r#"Hello <b>world</b> &amp; <i>friends</i>"#,
            );
        let runner = SequenceRunner::with_outputs(vec![
            output(&decorated),
            output(&decorated),
            output(&decorated),
        ]);
        let mut feed = NotificationDelivery::new(runner);
        let baseline = feed.poll_at(100);
        assert!(
            baseline.events.is_empty(),
            "existing history must not start unread"
        );
        assert_eq!(baseline.unread_count, 0);
        assert_eq!(
            baseline.notifications[0].urgency,
            NotificationUrgency::Critical
        );
        assert_eq!(
            baseline.notifications[0].entry.body,
            "Hello world & friends"
        );
        assert_eq!(baseline.notifications[0].display_timeout_ms, Some(1_500));
        assert!(baseline.notifications[0].display_expired);

        let next = feed.poll_at(1_100);
        assert!(next.events.is_empty());
        assert_eq!(
            next.notifications.len(),
            1,
            "display timeout is not history TTL"
        );
        assert_eq!(next.unread_count, 0);
    }

    #[test]
    fn transient_display_expiry_is_a_single_event() {
        let timed = history().replace(
            r#""id":{"type":"i","data":42},"#,
            r#""id":{"type":"i","data":42},"urgency":{"data":"LOW"},"timeout":{"data":1000000},"#,
        );
        let runner = SequenceRunner::with_outputs(vec![
            output(r#"{"data":[[]]}"#),
            output(&timed),
            output(&timed),
        ]);
        let mut feed = NotificationDelivery::new(runner);
        feed.poll_at(0);
        let added = feed.poll_at(1_000);
        assert_eq!(added.unread_count, 1);
        assert!(!added.notifications[0].display_expired);
        let expired = feed.poll_at(2_000);
        assert_eq!(
            expired.events,
            vec![NotificationDeliveryEvent::DisplayExpired { id: 42 }]
        );
        assert_eq!(expired.notifications.len(), 1);
        assert_eq!(feed.poll_at(2_200).unread_count, 1);
        assert!(feed.poll_at(2_200).events.is_empty());
    }

    #[test]
    fn unavailable_transport_retains_last_good_history_and_does_not_invent_removals() {
        let mut failed = output("");
        failed.status = 2;
        let mut feed = NotificationDelivery::new(SequenceRunner::with_outputs(vec![
            output(history()),
            failed,
        ]));
        assert_eq!(feed.poll_at(0).notifications.len(), 1);
        let degraded = feed.poll_at(1_000);
        assert_eq!(degraded.health, Health::Unavailable);
        assert_eq!(degraded.notifications.len(), 1);
        assert!(degraded.events.is_empty());
    }

    #[test]
    fn untrusted_markup_controls_and_action_ids_are_sanitized() {
        let value = bounded_text("A<b>safe</b>&amp;yes\u{202e}\u{1b}[31m");
        assert_eq!(value, "Asafe&yes");
        assert!(validate_action_name("reply_open:1").is_ok());
        assert!(validate_action_name("reply;touch /tmp/forged").is_err());
        assert!(validate_action_name("../illegal path").is_err());
        assert!(parse_history(&"x".repeat(MAX_HISTORY_BYTES + 1)).is_err());
    }

    #[test]
    fn action_labels_do_not_shift_key_pairing() {
        let input = history().replace(
            r#""actions":{"type":"as","data":["default","Open","reply","Reply"]}"#,
            r#""actions":{"type":"as","data":["default","Open now!","reply","Reply <b>today</b>"]}"#,
        );
        let (entries, issues) = parse_history(&input).unwrap();
        assert!(issues.is_empty());
        assert_eq!(entries[0].actions, vec!["default", "reply"]);
    }

    fn history_with_two_entries() -> String {
        let first = r#"{"id":{"data":42},"appname":{"data":"first"},"summary":{"data":"First"},"body":{"data":"old"}}"#;
        let second = r#"{"id":{"data":77},"appname":{"data":"second"},"summary":{"data":"Second"},"body":{"data":"new"}}"#;
        format!(r#"{{"data":[[{first},{second}]]}}"#)
    }

    #[test]
    fn clear_known_never_erases_new_arrivals_after_confirmation() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("dunstctl").arg("history"),
            output(&history_with_two_entries()),
        );
        runner.insert(
            CommandSpec::new("dunstctl").args(["history-rm", "42"]),
            output(""),
        );
        // A global history-clear or history-rm 77 has no fixture and would
        // fail the test. Entry 77 is the arrival after UI confirmation.
        let result = NotificationProvider::new(runner)
            .execute(NotificationAction::ClearKnown { ids: vec![42] })
            .unwrap();
        assert!(result.executed);
        assert!(result.message.contains("1 known"));
    }

    #[test]
    fn legacy_clear_removes_only_visible_snapshot_ids_not_global_history() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("dunstctl").arg("history"),
            output(&history_with_two_entries()),
        );
        runner.insert(
            CommandSpec::new("dunstctl").args(["history-rm", "42"]),
            output(""),
        );
        runner.insert(
            CommandSpec::new("dunstctl").args(["history-rm", "77"]),
            output(""),
        );
        assert!(
            NotificationProvider::new(runner)
                .execute(NotificationAction::Clear)
                .unwrap()
                .executed
        );
    }

    #[derive(Default)]
    struct SlowClearRunner {
        calls: usize,
        timeouts: Vec<Duration>,
    }

    impl CommandRunner for SlowClearRunner {
        fn run(
            &mut self,
            spec: &CommandSpec,
            limits: CommandLimits,
        ) -> Result<CommandOutput, ProviderError> {
            assert_eq!(spec.program, "dunstctl");
            assert_eq!(spec.args.first().map(String::as_str), Some("history-rm"));
            self.calls += 1;
            self.timeouts.push(limits.timeout);
            // Simulate a moderately slow IPC call. Each individual call
            // completes, but the aggregate may not exceed the one deadline.
            std::thread::sleep(Duration::from_millis(75));
            Ok(output(""))
        }
    }

    #[test]
    fn clear_uses_one_global_deadline_not_64_individual_timeouts() {
        let ids: Vec<u32> = (1..=8).collect();
        let snapshot = NotificationSnapshot {
            meta: SnapshotMeta::new(0, Health::Healthy, false, "dunstctl"),
            backend: "dunstctl".into(),
            notifications: ids
                .iter()
                .map(|id| NotificationEntry {
                    id: *id,
                    app: String::new(),
                    summary: String::new(),
                    body: String::new(),
                    actions: Vec::new(),
                    default_action: None,
                })
                .collect(),
            issues: Vec::new(),
        };
        let mut runner = SlowClearRunner::default();
        let started = Instant::now();
        let result = clear_exact_history_with_budget(
            &mut runner,
            &snapshot,
            &ids,
            Duration::from_millis(145),
        );
        assert!(result.is_err());
        assert!(runner.calls < ids.len());
        assert!(runner.timeouts.windows(2).all(|pair| pair[1] < pair[0]));
        assert!(started.elapsed() < Duration::from_millis(500));
    }

    #[test]
    fn clear_known_rejects_duplicates_missing_ids_and_unhealthy_snapshots() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("dunstctl").arg("history"),
            output(&history_with_two_entries()),
        );
        let mut provider = NotificationProvider::new(runner);
        let duplicate = provider.execute(NotificationAction::ClearKnown { ids: vec![42, 42] });
        assert!(duplicate.is_err());
        let absent = provider.execute(NotificationAction::ClearKnown { ids: vec![42, 99] });
        assert!(absent.is_err());
        let oversized = provider.execute(NotificationAction::ClearKnown {
            ids: vec![42; MAX_NOTIFICATIONS + 1],
        });
        assert!(oversized.is_err());
        let unavailable = NotificationProvider::new(FixtureCommandRunner::default())
            .execute(NotificationAction::ClearKnown { ids: vec![42] });
        assert!(unavailable.is_err());
    }

    #[test]
    fn non_atomic_dunst_action_is_disabled_by_default() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("dunstctl").arg("history"),
            output(history()),
        );
        let error = NotificationProvider::new(runner)
            .execute(NotificationAction::Invoke {
                id: 42,
                action: "reply".to_owned(),
            })
            .unwrap_err();
        assert_eq!(error.category, crate::ProviderErrorCategory::Unavailable);
        assert!(error.to_string().contains("non") || error.to_string().contains("atom"));
    }

    #[test]
    fn json_parser_rejects_trailing_or_deep_malformed_input() {
        assert!(JsonParser::new(r#"{"data":[]} trailing"#).parse().is_err());
        assert!(JsonParser::new(r#"{"data":[{"id":{"data":1.2}}]}"#)
            .parse()
            .is_err());
    }
}
