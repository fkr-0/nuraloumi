use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub weekday: u8,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarDay {
    pub date: CalendarDate,
    pub has_events: bool,
    pub event_count: u8,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarEvent {
    pub date: CalendarDate,
    pub time: Option<String>,
    pub summary: String,
    pub location: Option<String>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalendarSnapshot {
    pub meta: SnapshotMeta,
    pub today: CalendarDate,
    pub month_days: Vec<CalendarDay>,
    pub events: Vec<CalendarEvent>,
    pub issues: Vec<String>,
}

pub fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}
pub fn date_from_days(days: i64) -> CalendarDate {
    let weekday = (days + 3).rem_euclid(7) as u8;
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    CalendarDate {
        year: year.clamp(0, u16::MAX as i64) as u16,
        month: month as u8,
        day: day as u8,
        weekday,
    }
}
pub fn days_from_date(year: u16, month: u8, day: u8) -> i64 {
    let mut y = i64::from(year);
    y -= i64::from(month <= 2);
    let era = y / 400;
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (m + if m > 2 { -3 } else { 9 }) + 2) / 5 + i64::from(day) - 1;
    era * 146097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719468
}
pub fn month_grid(year: u16, month: u8, events: &[CalendarEvent]) -> Vec<CalendarDay> {
    let first = days_from_date(year, month, 1);
    let start = first - i64::from(date_from_days(first).weekday);
    (0..42)
        .map(|i| {
            let date = date_from_days(start + i);
            let count = events.iter().filter(|e| e.date == date).count().min(255) as u8;
            CalendarDay {
                date,
                has_events: count > 0,
                event_count: count,
            }
        })
        .collect()
}
fn parse_date(s: &str) -> Option<CalendarDate> {
    let digits: String = s.chars().filter(char::is_ascii_digit).collect();
    if digits.len() < 8 {
        return None;
    }
    let y = digits[..4].parse::<u16>().ok()?;
    let m = digits[4..6].parse::<u8>().ok()?;
    let d = digits[6..8].parse::<u8>().ok()?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return None;
    }
    Some(date_from_days(days_from_date(y, m, d)))
}
pub fn parse_khal(output: &str) -> Vec<CalendarEvent> {
    output
        .lines()
        .take(1024)
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let raw = parts.next()?;
            let date = if raw.contains('-') {
                parse_date(raw)?
            } else {
                return None;
            };
            let maybe_time = parts.next()?;
            let time = if maybe_time.len() == 5 && maybe_time.as_bytes()[2] == b':' {
                Some(maybe_time.to_owned())
            } else {
                None
            };
            let summary = if time.is_some() {
                parts.collect::<Vec<_>>().join(" ")
            } else {
                std::iter::once(maybe_time)
                    .chain(parts)
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            if summary.is_empty() {
                return None;
            }
            Some(CalendarEvent {
                date,
                time,
                summary,
                location: None,
            })
        })
        .collect()
}
fn unescape_ical(s: &str) -> String {
    s.replace("\\n", "\n")
        .replace("\\N", "\n")
        .replace("\\,", ",")
        .replace("\\;", ";")
        .replace("\\\\", "\\")
}
pub fn parse_ics(input: &str) -> Vec<CalendarEvent> {
    let mut unfolded: Vec<String> = Vec::new();
    for line in input.lines().take(8192) {
        let line = line.trim_end_matches('\r');
        if (line.starts_with(' ') || line.starts_with('\t')) && !unfolded.is_empty() {
            unfolded.last_mut().unwrap().push_str(&line[1..]);
        } else {
            unfolded.push(line.to_owned());
        }
    }
    let mut events = Vec::new();
    type IcalEntry = (Option<CalendarDate>, Option<String>, String, Option<String>);
    let mut current: Option<IcalEntry> = None;
    for line in unfolded {
        if line == "BEGIN:VEVENT" {
            current = Some((None, None, String::new(), None));
            continue;
        }
        if line == "END:VEVENT" {
            if let Some((Some(date), time, summary, location)) = current.take() {
                if !summary.is_empty() {
                    events.push(CalendarEvent {
                        date,
                        time,
                        summary,
                        location,
                    });
                }
            }
            continue;
        }
        let Some(ref mut event) = current else {
            continue;
        };
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        match key.split(';').next().unwrap_or("") {
            "DTSTART" => {
                event.0 = parse_date(value);
                if value.len() >= 13 && value.as_bytes().get(8) == Some(&b'T') {
                    event.1 = Some(format!("{}:{}", &value[9..11], &value[11..13]));
                }
            }
            "SUMMARY" => event.2 = unescape_ical(value),
            "LOCATION" => event.3 = Some(unescape_ical(value)),
            _ => {}
        }
    }
    events
}
pub struct CalendarProvider<R: CommandRunner = SystemCommandRunner> {
    pub runner: R,
    pub ical_path: Option<PathBuf>,
    pub timestamp_override: Option<u64>,
}
impl CalendarProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self {
            runner: SystemCommandRunner,
            ical_path: None,
            timestamp_override: None,
        }
    }
}
impl<R: CommandRunner> CalendarProvider<R> {
    pub fn with_runner(runner: R) -> Self {
        Self {
            runner,
            ical_path: None,
            timestamp_override: None,
        }
    }
}
impl<R: CommandRunner> Provider for CalendarProvider<R> {
    type Snapshot = CalendarSnapshot;
    fn snapshot(&mut self) -> Result<CalendarSnapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        let today = date_from_days((timestamp / 86_400_000) as i64);
        let mut issues = Vec::new();
        let spec = CommandSpec::new("khal").args([
            "list",
            "--format",
            "{start-date} {start-time} {title} {location}",
            "today",
            "31d",
        ]);
        let result = self
            .runner
            .run(&spec, CommandLimits::new(Duration::from_secs(2), 65536));
        let (mut events, source) = match result {
            Ok(output) if output.status == 0 => (parse_khal(&output.stdout), "khal"),
            _ => (Vec::new(), "calendar-grid"),
        };
        if source != "khal" {
            if let Some(ref path) = self.ical_path {
                match fs::read_dir(path) {
                    Ok(entries) => {
                        for entry in entries.flatten().take(128) {
                            if entry.path().extension().is_some_and(|ext| ext == "ics") {
                                match fs::metadata(entry.path()) {
                                    Ok(meta) if meta.len() <= 65536 => {
                                        match fs::read_to_string(entry.path()) {
                                            Ok(body) => events.extend(parse_ics(&body)),
                                            Err(e) => issues.push(format!("ics read: {e}")),
                                        }
                                    }
                                    _ => issues.push("ics exceeds size limit".into()),
                                }
                            }
                        }
                    }
                    Err(e) => issues.push(format!("ics directory: {e}")),
                }
            }
        }
        events.sort_by_key(|e| (e.date.year, e.date.month, e.date.day, e.time.clone()));
        let health = if issues.is_empty() {
            Health::Healthy
        } else {
            Health::Degraded
        };
        Ok(CalendarSnapshot {
            meta: SnapshotMeta::new(timestamp, health, false, source),
            today,
            month_days: month_grid(today.year, today.month, &events),
            events,
            issues,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn date_roundtrip() {
        for (y, m, d, w) in [(1970, 1, 1, 3), (2026, 11, 3, 1), (2000, 2, 29, 1)] {
            let date = date_from_days(days_from_date(y, m, d));
            assert_eq!(
                (date.year, date.month, date.day, date.weekday),
                (y, m, d, w)
            );
        }
    }
    #[test]
    fn parses_ics() {
        let e = parse_ics("BEGIN:VEVENT\nDTSTART:20261103T143000\nSUMMARY:Meeting\\, team\nLOCATION:HQ\nEND:VEVENT\n");
        assert_eq!(e[0].summary, "Meeting, team");
        assert_eq!(e[0].time.as_deref(), Some("14:30"));
    }
    #[test]
    fn parses_khal_fixture() {
        let e = parse_khal("2026-11-03 14:30 Appointment Office\n");
        assert_eq!(e.len(), 1);
        assert_eq!(e[0].summary, "Appointment Office");
    }
}
