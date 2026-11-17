use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForecastDay {
    pub label: String,
    pub high_c: i8,
    pub low_c: i8,
    pub condition: String,
    pub glyph: String,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeatherSnapshot {
    pub meta: SnapshotMeta,
    pub backend: String,
    pub temperature_c: Option<i8>,
    pub condition: Option<String>,
    pub condition_glyph: Option<String>,
    pub humidity_percent: Option<u8>,
    pub wind_kmh: Option<u16>,
    pub forecast: Vec<ForecastDay>,
    pub location: Option<String>,
    pub issues: Vec<String>,
}
fn number<T: std::str::FromStr>(object: &Value, field: &str) -> Option<T> {
    match object.get(field)? {
        Value::String(raw) => raw.parse().ok(),
        Value::Number(raw) => raw.to_string().parse().ok(),
        _ => None,
    }
}

fn description(object: &Value) -> String {
    object
        .get("weatherDesc")
        .and_then(Value::as_array)
        .and_then(|descriptions| descriptions.first())
        .and_then(|description| description.get("value"))
        .and_then(Value::as_str)
        .unwrap_or("Unknown")
        .to_owned()
}

fn glyph(code: u16) -> &'static str {
    match code {
        113 => "☀",
        116 => "⛅",
        119 | 122 => "☁",
        143 | 248 | 260 => "🌫",
        179 | 182 | 185 | 227 | 230 | 323 | 326 | 329 | 332 | 335 | 338 | 368 | 371 | 374 | 377
        | 392 | 395 => "❄",
        200 | 386 | 389 => "⚡",
        _ => "🌧",
    }
}
pub fn parse_wttr(body: &str, now: u64) -> Result<WeatherSnapshot, ProviderError> {
    // The command runner already caps stdout; preserve that same bound for
    // public parser callers and adversarial fixtures.
    if body.len() > 64 * 1024 {
        return Err(ProviderError::parse("weather response exceeds 64 KiB"));
    }
    let document: Value = serde_json::from_str(body)
        .map_err(|error| ProviderError::parse(format!("invalid weather JSON: {error}")))?;
    // Resolve fields from the root JSON object. Searching the raw body for
    // matching key substrings accepted nested spoofing and malformed JSON.
    let current = document
        .get("current_condition")
        .and_then(Value::as_array)
        .and_then(|conditions| conditions.first())
        .filter(|condition| condition.is_object())
        .ok_or_else(|| ProviderError::parse("missing current_condition object"))?;
    let temperature_c: i8 = number(current, "temp_C")
        .ok_or_else(|| ProviderError::parse("missing or invalid current temperature"))?;

    let forecast = document
        .get("weather")
        .and_then(Value::as_array)
        .into_iter()
        .flat_map(|days| days.iter())
        .take(3)
        .enumerate()
        .filter_map(|(index, day)| {
            // Do not fabricate a 0 °C forecast from missing or bad data.
            let high_c = number(day, "maxtempC")?;
            let low_c = number(day, "mintempC")?;
            let details = day
                .get("hourly")
                .and_then(Value::as_array)
                .and_then(|hours| hours.get(4).or_else(|| hours.first()))
                .unwrap_or(current);
            Some(ForecastDay {
                label: match index {
                    0 => "Today",
                    1 => "Tomorrow",
                    _ => "Day 3",
                }
                .into(),
                high_c,
                low_c,
                condition: description(details),
                glyph: glyph(number(details, "weatherCode").unwrap_or(119)).into(),
            })
        })
        .collect();

    let location = document
        .get("nearest_area")
        .and_then(Value::as_array)
        .and_then(|areas| areas.first())
        .and_then(|area| area.get("areaName"))
        .and_then(Value::as_array)
        .and_then(|names| names.first())
        .and_then(|name| name.get("value"))
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok(WeatherSnapshot {
        meta: SnapshotMeta::new(now, Health::Healthy, false, "wttr.in"),
        backend: "wttr.in".into(),
        temperature_c: Some(temperature_c),
        condition: Some(description(current)),
        condition_glyph: Some(glyph(number(current, "weatherCode").unwrap_or(119)).into()),
        humidity_percent: number(current, "humidity"),
        wind_kmh: number(current, "windspeedKmph"),
        forecast,
        location,
        issues: Vec::new(),
    })
}

pub struct WeatherProvider<R: CommandRunner = SystemCommandRunner> {
    pub runner: R,
    pub url: String,
    pub location: Option<String>,
    pub cache_seconds: u64,
    pub timestamp_override: Option<u64>,
    cache: Option<WeatherSnapshot>,
}
impl WeatherProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self::with_runner(SystemCommandRunner)
    }
}
impl<R: CommandRunner> WeatherProvider<R> {
    pub fn with_runner(runner: R) -> Self {
        Self {
            runner,
            url: "https://wttr.in/?format=j1".into(),
            location: None,
            cache_seconds: 1800,
            timestamp_override: None,
            cache: None,
        }
    }
    pub fn with_fixture(runner: R, snapshot: WeatherSnapshot) -> Self {
        let mut this = Self::with_runner(runner);
        this.cache = Some(snapshot);
        this
    }
    pub fn refresh(&mut self) -> WeatherSnapshot {
        let now = timestamp_ms(self.timestamp_override);
        let mut url = self.url.clone();
        if let Some(ref location) = self.location {
            if self.url == "https://wttr.in/?format=j1" {
                url = format!("https://wttr.in/{}?format=j1", location.replace(' ', "+"));
            }
        }
        let result = self
            .runner
            .run(
                &CommandSpec::new("curl").args(["-fsS", "--max-time", "5", "--", &url]),
                CommandLimits::new(Duration::from_secs(5), 65536),
            )
            .and_then(|out| {
                if out.status == 0 {
                    parse_wttr(&out.stdout, now)
                } else {
                    Err(ProviderError::backend(format!("curl exit {}", out.status)))
                }
            });
        match result {
            Ok(snapshot) => {
                self.cache = Some(snapshot.clone());
                snapshot
            }
            Err(error) => {
                if let Some(mut cached) = self.cache.clone() {
                    cached.meta.health = Health::Degraded;
                    cached.meta.stale = true;
                    let minutes = now.saturating_sub(cached.meta.timestamp_ms) / 60000;
                    cached.issues.push(format!(
                        "last updated {minutes} min ago: {}",
                        error.diagnostic()
                    ));
                    cached
                } else {
                    WeatherSnapshot {
                        meta: SnapshotMeta::new(now, Health::Unavailable, false, "wttr.in"),
                        backend: "wttr.in".into(),
                        temperature_c: None,
                        condition: None,
                        condition_glyph: None,
                        humidity_percent: None,
                        wind_kmh: None,
                        forecast: Vec::new(),
                        location: None,
                        issues: vec![error.diagnostic()],
                    }
                }
            }
        }
    }
}
impl<R: CommandRunner> Provider for WeatherProvider<R> {
    type Snapshot = WeatherSnapshot;
    fn snapshot(&mut self) -> Result<WeatherSnapshot, ProviderError> {
        let now = timestamp_ms(self.timestamp_override);
        if let Some(cache) = &self.cache {
            if now.saturating_sub(cache.meta.timestamp_ms) < self.cache_seconds.saturating_mul(1000)
            {
                return Ok(cache.clone());
            }
        }
        // Snapshot is deliberately nonblocking: explicit refresh() performs the bounded
        // command, and should be scheduled on a worker or invoked from an explicit action.
        if let Some(mut cache) = self.cache.clone() {
            cache.meta.stale = true;
            cache.meta.health = Health::Degraded;
            cache.issues.push(format!(
                "last updated {} min ago; refresh required",
                now.saturating_sub(cache.meta.timestamp_ms) / 60000
            ));
            return Ok(cache);
        }
        Ok(WeatherSnapshot {
            meta: SnapshotMeta::new(now, Health::Unavailable, false, "wttr.in"),
            backend: "wttr.in".into(),
            temperature_c: None,
            condition: None,
            condition_glyph: None,
            humidity_percent: None,
            wind_kmh: None,
            forecast: Vec::new(),
            location: self.location.clone(),
            issues: vec!["weather not fetched; explicit refresh required".into()],
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::FixtureCommandRunner;
    #[test]
    fn parses_json() {
        let v = r#"{"current_condition":[{"temp_C":"12","weatherCode":"113","humidity":"71","windspeedKmph":"8","weatherDesc":[{"value":"Sunny"}]}],"weather":[{"maxtempC":"14","mintempC":"7"}]}"#;
        let s = parse_wttr(v, 42).unwrap();
        assert_eq!(s.temperature_c, Some(12));
        assert_eq!(s.condition_glyph.as_deref(), Some("☀"));
    }
    #[test]
    fn stale_fallback() {
        let snapshot = parse_wttr(r#"{"current_condition":[{"temp_C":"12"}]}"#, 1000).unwrap();
        let mut p = WeatherProvider::with_fixture(FixtureCommandRunner::default(), snapshot);
        p.timestamp_override = Some(2_000_000);
        p.cache_seconds = 1;
        let s = p.refresh();
        assert!(s.meta.stale);
        assert_eq!(s.temperature_c, Some(12));
    }
}
