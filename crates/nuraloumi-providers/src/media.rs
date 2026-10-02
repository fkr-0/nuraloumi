use crate::command::{CommandLimits, CommandRunner, CommandSpec, SystemCommandRunner};
use crate::common::{timestamp_ms, ActionProvider, ActionResult, Health, Provider, SnapshotMeta};
use crate::error::ProviderError;
use crate::notifications::{JsonParser, JsonValue};
use std::time::Duration;

const SNAPSHOT_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(2), 32 * 1024);
const ACTION_LIMITS: CommandLimits = CommandLimits::new(Duration::from_secs(3), 16 * 1024);
const METADATA_FORMAT: &str = "{{artist}}\u{1f}{{title}}";
const MAX_TEXT_CHARS: usize = 192;
const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const MPRIS_PLAYER: &str = "org.mpris.MediaPlayer2.Player";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaPlayer {
    pub id: String,
    pub status: String,
    pub artist: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaSnapshot {
    pub meta: SnapshotMeta,
    pub backend: String,
    pub players: Vec<MediaPlayer>,
    pub current_player: Option<String>,
    pub issues: Vec<String>,
}

impl MediaSnapshot {
    pub fn unavailable(timestamp: u64, backend: &str, issue: String) -> Self {
        Self {
            meta: SnapshotMeta::new(timestamp, Health::Unavailable, false, backend),
            backend: backend.to_owned(),
            players: Vec::new(),
            current_player: None,
            issues: vec![issue],
        }
    }

    pub fn current(&self) -> Option<&MediaPlayer> {
        let id = self.current_player.as_deref()?;
        self.players.iter().find(|player| player.id == id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MediaAction {
    PlayPause { player: String },
    Previous { player: String },
    Next { player: String },
}

pub struct MediaProvider<R> {
    runner: R,
    timestamp_override: Option<u64>,
}

impl MediaProvider<SystemCommandRunner> {
    pub fn system() -> Self {
        Self::new(SystemCommandRunner)
    }
}

impl<R> MediaProvider<R> {
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

impl<R: CommandRunner> MediaProvider<R> {
    fn playerctl_snapshot(&mut self, timestamp: u64) -> Option<MediaSnapshot> {
        let list = self
            .runner
            .run(
                &CommandSpec::new("playerctl").arg("--list-all"),
                SNAPSHOT_LIMITS,
            )
            .ok()
            .filter(|output| output.status == 0)?;

        let mut ids = list
            .stdout
            .lines()
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .filter_map(|id| validate_player_id(id).ok().map(|()| id.to_owned()))
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();
        if ids.is_empty() {
            return None;
        }

        let mut players = Vec::new();
        let mut issues = Vec::new();
        for id in ids {
            let selector = format!("--player={id}");
            let status = self
                .runner
                .run(
                    &CommandSpec::new("playerctl").args([selector.clone(), "status".to_owned()]),
                    SNAPSHOT_LIMITS,
                )
                .ok()
                .filter(|output| output.status == 0)
                .map(|output| bounded_text(output.stdout.trim()))
                .filter(|status| !status.is_empty());

            let metadata = self
                .runner
                .run(
                    &CommandSpec::new("playerctl").args([
                        selector,
                        "metadata".to_owned(),
                        "--format".to_owned(),
                        METADATA_FORMAT.to_owned(),
                    ]),
                    SNAPSHOT_LIMITS,
                )
                .ok()
                .filter(|output| output.status == 0)
                .map(|output| output.stdout.trim_end().to_owned());

            let (artist, title) = metadata
                .as_deref()
                .map(parse_playerctl_metadata)
                .unwrap_or((None, None));
            let Some(status) = status else {
                issues.push(format!("player {id:?} did not expose playback status"));
                continue;
            };
            players.push(MediaPlayer {
                id,
                status,
                artist,
                title,
            });
        }

        Some(snapshot_from_players(
            timestamp,
            "playerctl",
            players,
            issues,
        ))
    }

    fn busctl_snapshot(&mut self, timestamp: u64) -> MediaSnapshot {
        let list = match self.runner.run(
            &CommandSpec::new("busctl").args(["--user", "list", "--no-pager", "--no-legend"]),
            SNAPSHOT_LIMITS,
        ) {
            Ok(output) if output.status == 0 => output,
            Ok(output) => {
                return MediaSnapshot::unavailable(
                    timestamp,
                    "mpris",
                    format!(
                        "playerctl unavailable and busctl list exited with status {}",
                        output.status
                    ),
                )
            }
            Err(error) => {
                return MediaSnapshot::unavailable(
                    timestamp,
                    "mpris",
                    format!(
                        "neither playerctl nor busctl MPRIS discovery is available: {}",
                        error.diagnostic()
                    ),
                )
            }
        };

        let mut ids = list
            .stdout
            .lines()
            .filter_map(|line| line.split_whitespace().next())
            .filter(|id| id.starts_with(MPRIS_PREFIX))
            .filter_map(|id| validate_player_id(id).ok().map(|()| id.to_owned()))
            .collect::<Vec<_>>();
        ids.sort();
        ids.dedup();

        if ids.is_empty() {
            return MediaSnapshot::unavailable(
                timestamp,
                "busctl-mpris",
                "no MPRIS players are currently available".to_owned(),
            );
        }

        let mut players = Vec::new();
        let mut issues = Vec::new();
        for id in ids {
            let status = self
                .runner
                .run(
                    &CommandSpec::new("busctl").args([
                        "--user",
                        "--json=short",
                        "get-property",
                        &id,
                        MPRIS_PATH,
                        MPRIS_PLAYER,
                        "PlaybackStatus",
                    ]),
                    SNAPSHOT_LIMITS,
                )
                .ok()
                .filter(|output| output.status == 0)
                .and_then(|output| parse_busctl_string(&output.stdout).ok())
                .map(|status| bounded_text(&status));

            let metadata = self
                .runner
                .run(
                    &CommandSpec::new("busctl").args([
                        "--user",
                        "--json=short",
                        "get-property",
                        &id,
                        MPRIS_PATH,
                        MPRIS_PLAYER,
                        "Metadata",
                    ]),
                    SNAPSHOT_LIMITS,
                )
                .ok()
                .filter(|output| output.status == 0)
                .and_then(|output| parse_busctl_metadata(&output.stdout).ok())
                .unwrap_or((None, None));

            let Some(status) = status.filter(|value| !value.is_empty()) else {
                issues.push(format!("player {id:?} did not expose playback status"));
                continue;
            };
            players.push(MediaPlayer {
                id,
                status,
                artist: metadata.0,
                title: metadata.1,
            });
        }

        snapshot_from_players(timestamp, "busctl-mpris", players, issues)
    }
}

impl<R: CommandRunner> Provider for MediaProvider<R> {
    type Snapshot = MediaSnapshot;

    fn snapshot(&mut self) -> Result<Self::Snapshot, ProviderError> {
        let timestamp = timestamp_ms(self.timestamp_override);
        if let Some(snapshot) = self.playerctl_snapshot(timestamp) {
            return Ok(snapshot);
        }
        Ok(self.busctl_snapshot(timestamp))
    }
}

impl<R: CommandRunner> ActionProvider<MediaAction> for MediaProvider<R> {
    fn execute(&mut self, action: MediaAction) -> Result<ActionResult, ProviderError> {
        let (player, playerctl_command, busctl_method) = match action {
            MediaAction::PlayPause { player } => (player, "play-pause", "PlayPause"),
            MediaAction::Previous { player } => (player, "previous", "Previous"),
            MediaAction::Next { player } => (player, "next", "Next"),
        };
        validate_player_id(&player)?;

        let snapshot = self.snapshot()?;
        if !snapshot
            .players
            .iter()
            .any(|candidate| candidate.id == player)
        {
            return Err(ProviderError::unavailable(format!(
                "MPRIS player {player:?} is no longer available"
            )));
        }

        let (spec, backend_action) = if snapshot.backend == "playerctl" {
            (
                CommandSpec::new("playerctl")
                    .args([format!("--player={player}"), playerctl_command.to_owned()]),
                playerctl_command,
            )
        } else {
            (
                CommandSpec::new("busctl").args([
                    "--user",
                    "call",
                    &player,
                    MPRIS_PATH,
                    MPRIS_PLAYER,
                    busctl_method,
                ]),
                busctl_method,
            )
        };

        let output = self.runner.run(&spec, ACTION_LIMITS)?;
        if output.status != 0 {
            return Err(ProviderError::backend(format!(
                "MPRIS action {backend_action} exited with status {}",
                output.status
            )));
        }
        Ok(ActionResult::executed(format!(
            "media action {backend_action} completed for {player}"
        )))
    }
}

fn snapshot_from_players(
    timestamp: u64,
    backend: &str,
    players: Vec<MediaPlayer>,
    issues: Vec<String>,
) -> MediaSnapshot {
    let current_player = players
        .iter()
        .find(|player| player.status.eq_ignore_ascii_case("playing"))
        .or_else(|| players.first())
        .map(|player| player.id.clone());
    let health = if players.is_empty() {
        Health::Unavailable
    } else if issues.is_empty() {
        Health::Healthy
    } else {
        Health::Degraded
    };

    MediaSnapshot {
        meta: SnapshotMeta::new(timestamp, health, false, backend),
        backend: backend.to_owned(),
        players,
        current_player,
        issues,
    }
}

fn parse_playerctl_metadata(value: &str) -> (Option<String>, Option<String>) {
    let mut fields = value.splitn(2, '\u{1f}');
    let artist = fields
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(bounded_text);
    let title = fields
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(bounded_text);
    (artist, title)
}

fn parse_busctl_string(value: &str) -> Result<String, ProviderError> {
    let root = JsonParser::new(value).parse()?;
    root.object()
        .and_then(|object| object.get("data"))
        .and_then(JsonValue::string)
        .map(ToOwned::to_owned)
        .ok_or_else(|| ProviderError::parse("busctl property did not contain string data"))
}

fn parse_busctl_metadata(value: &str) -> Result<(Option<String>, Option<String>), ProviderError> {
    let root = JsonParser::new(value).parse()?;
    let data = root
        .object()
        .and_then(|object| object.get("data"))
        .and_then(JsonValue::object)
        .ok_or_else(|| ProviderError::parse("busctl metadata did not contain a data object"))?;

    let title = typed_string(data.get("xesam:title")).map(|value| bounded_text(&value));
    let artist = typed_strings(data.get("xesam:artist"))
        .into_iter()
        .next()
        .map(|value| bounded_text(&value));
    Ok((artist, title))
}

fn typed_data(value: Option<&JsonValue>) -> Option<&JsonValue> {
    value?.object()?.get("data")
}

fn typed_string(value: Option<&JsonValue>) -> Option<String> {
    typed_data(value)?.string().map(ToOwned::to_owned)
}

fn typed_strings(value: Option<&JsonValue>) -> Vec<String> {
    typed_data(value)
        .and_then(JsonValue::array)
        .map(|values| {
            values
                .iter()
                .filter_map(JsonValue::string)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn bounded_text(value: &str) -> String {
    if value.chars().count() <= MAX_TEXT_CHARS {
        return value.to_owned();
    }
    let mut text = value.chars().take(MAX_TEXT_CHARS - 1).collect::<String>();
    text.push('…');
    text
}

fn validate_player_id(value: &str) -> Result<(), ProviderError> {
    if value.is_empty()
        || value.len() > 256
        || value
            .chars()
            .any(|ch| ch == '\0' || ch == '\n' || ch == '\r')
    {
        return Err(ProviderError::parse("invalid MPRIS player id"));
    }
    Ok(())
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

    #[test]
    fn missing_backends_are_unavailable_not_fatal() {
        let snapshot = MediaProvider::new(FixtureCommandRunner::default())
            .with_timestamp(Some(7))
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.meta.health, Health::Unavailable);
        assert!(snapshot.players.is_empty());
    }

    #[test]
    fn playing_player_wins_current_selection_and_playerctl_actions_use_exact_argv() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("playerctl").arg("--list-all"),
            output("mpv\nvlc\n"),
        );
        for (id, status, artist, title) in [
            ("mpv", "Paused\n", "A", "One"),
            ("vlc", "Playing\n", "B", "Two"),
        ] {
            runner.insert(
                CommandSpec::new("playerctl").args([format!("--player={id}"), "status".to_owned()]),
                output(status),
            );
            runner.insert(
                CommandSpec::new("playerctl").args([
                    format!("--player={id}"),
                    "metadata".to_owned(),
                    "--format".to_owned(),
                    METADATA_FORMAT.to_owned(),
                ]),
                output(&format!("{artist}\u{1f}{title}\n")),
            );
        }
        runner.insert(
            CommandSpec::new("playerctl")
                .args(["--player=vlc".to_owned(), "play-pause".to_owned()]),
            output(""),
        );

        let mut provider = MediaProvider::new(runner).with_timestamp(Some(8));
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.backend, "playerctl");
        assert_eq!(snapshot.current_player.as_deref(), Some("vlc"));
        assert_eq!(
            snapshot.current().and_then(|p| p.title.as_deref()),
            Some("Two")
        );
        assert!(
            provider
                .execute(MediaAction::PlayPause {
                    player: "vlc".to_owned()
                })
                .unwrap()
                .executed
        );
    }

    #[test]
    fn busctl_fallback_exposes_metadata_and_controls_without_playerctl() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("busctl").args(["--user", "list", "--no-pager", "--no-legend"]),
            output("org.mpris.MediaPlayer2.vlc 12 user - - - - - -\n"),
        );
        runner.insert(
            CommandSpec::new("busctl").args([
                "--user",
                "--json=short",
                "get-property",
                "org.mpris.MediaPlayer2.vlc",
                MPRIS_PATH,
                MPRIS_PLAYER,
                "PlaybackStatus",
            ]),
            output(r#"{"type":"s","data":"Playing"}"#),
        );
        runner.insert(
            CommandSpec::new("busctl").args([
                "--user",
                "--json=short",
                "get-property",
                "org.mpris.MediaPlayer2.vlc",
                MPRIS_PATH,
                MPRIS_PLAYER,
                "Metadata",
            ]),
            output(
                r#"{"type":"a{sv}","data":{"xesam:title":{"type":"s","data":"Track"},"xesam:artist":{"type":"as","data":["Artist"]}}}"#,
            ),
        );
        runner.insert(
            CommandSpec::new("busctl").args([
                "--user",
                "call",
                "org.mpris.MediaPlayer2.vlc",
                MPRIS_PATH,
                MPRIS_PLAYER,
                "Next",
            ]),
            output(""),
        );

        let mut provider = MediaProvider::new(runner).with_timestamp(Some(9));
        let snapshot = provider.snapshot().unwrap();
        assert_eq!(snapshot.backend, "busctl-mpris");
        assert_eq!(
            snapshot
                .current()
                .and_then(|player| player.title.as_deref()),
            Some("Track")
        );
        assert_eq!(
            snapshot
                .current()
                .and_then(|player| player.artist.as_deref()),
            Some("Artist")
        );
        assert!(
            provider
                .execute(MediaAction::Next {
                    player: "org.mpris.MediaPlayer2.vlc".to_owned()
                })
                .unwrap()
                .executed
        );
    }

    #[test]
    fn stale_or_forged_player_ids_fail_closed() {
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("playerctl").arg("--list-all"),
            output("mpv\n"),
        );
        runner.insert(
            CommandSpec::new("playerctl").args(["--player=mpv".to_owned(), "status".to_owned()]),
            output("Paused\n"),
        );
        runner.insert(
            CommandSpec::new("playerctl").args([
                "--player=mpv".to_owned(),
                "metadata".to_owned(),
                "--format".to_owned(),
                METADATA_FORMAT.to_owned(),
            ]),
            output("\u{1f}Track\n"),
        );

        let error = MediaProvider::new(runner)
            .execute(MediaAction::Next {
                player: "vlc;touch /tmp/nope".to_owned(),
            })
            .unwrap_err();
        assert_eq!(error.category, crate::ProviderErrorCategory::Unavailable);
    }
}
