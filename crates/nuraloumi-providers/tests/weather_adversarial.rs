//! Offline adversarial qualification for NuraLoumi 0.0.8 weather data.
//! No HTTP, Wayland, device or system commands are executed.

use nuraloumi_providers::command::{CommandOutput, CommandSpec, FixtureCommandRunner};
use nuraloumi_providers::weather::{parse_wttr, WeatherProvider};

const FULL: &str = r#"{
    "untrusted": {
        "current_condition": [{"temp_C": "44"}],
        "weather": [{"maxtempC":"99","mintempC":"99"}]
    },
    "current_condition": [{
        "temp_C": "12",
        "humidity": "70",
        "windspeedKmph": "9",
        "weatherCode": "113",
        "weatherDesc": [{"value": "Sunny"}]
    }],
    "weather": [
        {"maxtempC":"17","mintempC":"7","hourly":[{"weatherCode":"116","weatherDesc":[{"value":"Cloudy"}]}]},
        {"maxtempC":"13","mintempC":"4","hourly":[{"weatherCode":"119","weatherDesc":[{"value":"Rain"}]}]},
        {"maxtempC":"15","mintempC":"6","hourly":[{"weatherCode":"113","weatherDesc":[{"value":"Bright"}]}]},
        {"maxtempC":"19","mintempC":"8"}
    ],
    "nearest_area": [{"areaName": [{"value": "Fixture City"}]}]
}"#;

#[test]
fn top_level_fields_are_not_shadowed_by_nested_json() {
    let weather = parse_wttr(FULL, 42).unwrap();
    assert_eq!(weather.temperature_c, Some(12));
    assert_eq!(weather.humidity_percent, Some(70));
    assert_eq!(weather.wind_kmh, Some(9));
    assert_eq!(weather.location.as_deref(), Some("Fixture City"));
    assert_eq!(weather.condition.as_deref(), Some("Sunny"));
    assert_eq!(weather.condition_glyph.as_deref(), Some("☀"));
    assert_eq!(weather.forecast.len(), 3);
    assert_eq!(weather.forecast[0].high_c, 17);
    assert_eq!(weather.forecast[0].low_c, 7);
    assert_eq!(weather.forecast[1].label, "Tomorrow");
}

#[test]
fn malformed_or_wrong_shaped_json_never_claims_healthy_weather() {
    for invalid in [
        r#"{"current_condition":[{"temp_C":"12"}]"#,
        r#"{"current_condition":{"temp_C":"12"},"weather":[{"temp_C":"9"}]}"#,
        r#"{"current_condition":[{}]}"#,
        r#"{"current_condition":[{"temp_C":"not-a-number"}]}"#,
        r#"{"current_condition":[{"temp_C":99999}]}"#,
        "[]",
        "",
    ] {
        assert!(
            parse_wttr(invalid, 123).is_err(),
            "invalid weather must be rejected: {invalid}"
        );
    }
}

#[test]
fn bounded_json_rejects_oversized_input() {
    let oversized = format!("{}{}", FULL, " ".repeat(65_537));
    assert!(parse_wttr(&oversized, 123).is_err());
}

#[test]
fn explicit_refresh_uses_positional_url_after_end_of_curl_options() {
    let mut runner = FixtureCommandRunner::default();
    runner.insert(
        CommandSpec::new("curl").args([
            "-fsS",
            "--max-time",
            "5",
            "--",
            "https://wttr.in/?format=j1",
        ]),
        CommandOutput {
            status: 0,
            stdout: FULL.into(),
            stderr: String::new(),
        },
    );
    let mut provider = WeatherProvider::with_runner(runner);
    let snapshot = provider.refresh();
    assert_eq!(snapshot.temperature_c, Some(12));
    assert_eq!(snapshot.forecast.len(), 3);
}
