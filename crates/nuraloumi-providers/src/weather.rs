use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
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
fn extract<'a>(input: &'a str, key: &str) -> Option<&'a str> {
    let needle = format!(r#""{key}""#);
    let pos = input.find(&needle)? + needle.len();
    let tail = input[pos..].trim_start();
    let tail = tail.strip_prefix(':')?.trim_start();
    if tail.starts_with('"') {
        let bytes = tail.as_bytes();
        let mut escape = false;
        for i in 1..bytes.len() {
            if escape {
                escape = false;
                continue;
            }
            if bytes[i] == b'\\' {
                escape = true;
                continue;
            }
            if bytes[i] == b'"' {
                return Some(&tail[1..i]);
            }
        }
        None
    } else {
        let end = tail.find([',', '}', ']'])?;
        Some(tail[..end].trim())
    }
}
fn objects<'a>(input: &'a str, key: &str) -> Vec<&'a str> {
    let needle = format!(r#""{key}""#);
    let Some(pos) = input.find(&needle) else {
        return Vec::new();
    };
    let Some(array) = input[pos + needle.len()..]
        .find('[')
        .map(|i| pos + needle.len() + i + 1)
    else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut depth = 0;
    let mut begin = 0;
    let mut quoted = false;
    let mut escaped = false;
    for (i, c) in input[array..].char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if c == '\\' && quoted {
            escaped = true;
            continue;
        }
        if c == '"' {
            quoted = !quoted;
            continue;
        }
        if quoted {
            continue;
        }
        match c {
            '{' => {
                if depth == 0 {
                    begin = i;
                }
                depth += 1;
            }
            '}' => {
                if depth > 0 {
                    depth -= 1;
                    if depth == 0 {
                        out.push(&input[array + begin..array + i + 1]);
                    }
                }
            }
            ']' if depth == 0 => break,
            _ => {}
        }
    }
    out
}
fn num<T: std::str::FromStr>(s: &str, k: &str) -> Option<T> {
    extract(s, k)?.parse().ok()
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
fn description(s: &str) -> String {
    extract(s, "value").unwrap_or("Unknown").to_owned()
}
pub fn parse_wttr(body: &str, now: u64) -> Result<WeatherSnapshot, ProviderError> {
    let current = objects(body, "current_condition")
        .into_iter()
        .next()
        .ok_or_else(|| ProviderError::parse("missing current_condition"))?;
    let forecast = objects(body, "weather")
        .into_iter()
        .take(3)
        .enumerate()
        .map(|(i, day)| {
            let hourly = objects(day, "hourly");
            let details = hourly
                .get(4)
                .or_else(|| hourly.first())
                .copied()
                .unwrap_or(current);
            ForecastDay {
                label: match i {
                    0 => "Today",
                    1 => "Tomorrow",
                    _ => "Day 3",
                }
                .into(),
                high_c: num(day, "maxtempC").unwrap_or(0),
                low_c: num(day, "mintempC").unwrap_or(0),
                condition: description(details),
                glyph: glyph(num(details, "weatherCode").unwrap_or(119)).into(),
            }
        })
        .collect();
    let location = objects(body, "nearest_area")
        .into_iter()
        .next()
        .and_then(|a| extract(a, "value"))
        .map(str::to_owned);
    Ok(WeatherSnapshot {
        meta: SnapshotMeta::new(now, Health::Healthy, false, "wttr.in"),
        backend: "wttr.in".into(),
        temperature_c: num(current, "temp_C"),
        condition: Some(description(current)),
        condition_glyph: Some(glyph(num(current, "weatherCode").unwrap_or(119)).into()),
        humidity_percent: num(current, "humidity"),
        wind_kmh: num(current, "windspeedKmph"),
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
                &CommandSpec::new("curl").args(["-fsS", "--max-time", "5", &url]),
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
