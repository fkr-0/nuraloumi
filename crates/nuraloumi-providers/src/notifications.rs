use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::collections::BTreeMap;
use std::time::Duration;

const SNAPSHOT_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(2), 256 * 1024);
const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(3), 32 * 1024);
const MAX_NOTIFICATIONS: usize = 64;
const MAX_ACTIONS: usize = 16;
const MAX_JSON_DEPTH: usize = 32;
const MAX_TEXT_CHARS: usize = 240;

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
    Redisplay { id: u32 },
    Remove { id: u32 },
    Invoke { id: u32, action: String },
    Clear,
}

pub struct NotificationProvider<R> {
    runner: R,
    timestamp_override: Option<u64>,
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
        }
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
                run_action(&mut self.runner, ["history-clear".to_owned()])?;
                Ok(ActionResult::executed("notification history cleared"))
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

                // Dunst addresses action invocation by displayed position rather
                // than history id. Redisplay the exact history entry first, then
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
    let output = runner.run(&CommandSpec::new("dunstctl").args(args), ACTION_LIMITS)?;
    if output.status != 0 {
        return Err(ProviderError::backend(format!(
            "dunstctl action exited with status {}",
            output.status
        )));
    }
    Ok(())
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
        || value.len() > 256
        || value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(ProviderError::parse("invalid notification action name"));
    }
    Ok(())
}

fn parse_history(text: &str) -> Result<(Vec<NotificationEntry>, Vec<String>), ProviderError> {
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
    notifications: &mut Vec<NotificationEntry>,
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
            issues.push("dunst history contained a non-object entry".to_owned());
            continue;
        };
        let Some(id) = variant_i64(object.get("id")).and_then(|id| u32::try_from(id).ok()) else {
            issues.push("dunst history entry missing valid id".to_owned());
            continue;
        };

        let app = bounded_text(&variant_string(object.get("appname")).unwrap_or_default());
        let summary = bounded_text(&variant_string(object.get("summary")).unwrap_or_default());
        let body = bounded_text(&variant_string(object.get("body")).unwrap_or_default());
        let default_action = variant_string(object.get("default_action_name"))
            .filter(|value| !value.trim().is_empty())
            .map(|value| bounded_text(&value));
        let mut actions = variant_action_names(object.get("actions"));
        if let Some(default_action) = &default_action {
            if !actions.iter().any(|action| action == default_action) {
                actions.push(default_action.clone());
            }
        }
        actions.truncate(MAX_ACTIONS);

        notifications.push(NotificationEntry {
            id,
            app,
            summary,
            body,
            actions,
            default_action,
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
        .map(bounded_text)
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();

    // D-Bus notification actions are conventionally alternating key/label
    // pairs. Dunst versions that export only action keys are also accepted.
    let candidates = if raw.len() >= 2 && raw.len() % 2 == 0 {
        raw.into_iter().step_by(2).collect::<Vec<_>>()
    } else {
        raw
    };
    candidates.into_iter().take(MAX_ACTIONS).collect()
}

fn bounded_text(value: &str) -> String {
    if value.chars().count() <= MAX_TEXT_CHARS {
        return value.to_owned();
    }
    let mut truncated = value.chars().take(MAX_TEXT_CHARS - 1).collect::<String>();
    truncated.push('…');
    truncated
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
        assert_eq!(error.category, crate::ProviderErrorCategory::Unavailable);
    }

    #[test]
    fn json_parser_rejects_trailing_or_deep_malformed_input() {
        assert!(JsonParser::new(r#"{"data":[]} trailing"#).parse().is_err());
        assert!(JsonParser::new(r#"{"data":[{"id":{"data":1.2}}]}"#)
            .parse()
            .is_err());
    }
}
