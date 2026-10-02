use nuraloumi_wayland::{ToplevelCapabilities, ToplevelId, ToplevelInfo, WaylandBackend};
use serde::{Deserialize, Serialize};

use crate::{MenuAction, WindowEntry};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowControlCapabilities {
    pub list: bool,
    pub focus: bool,
    pub fullscreen: bool,
    pub close: bool,
}

impl WindowControlCapabilities {
    pub const fn unavailable() -> Self {
        Self {
            list: false,
            focus: false,
            fullscreen: false,
            close: false,
        }
    }
}

impl From<ToplevelCapabilities> for WindowControlCapabilities {
    fn from(value: ToplevelCapabilities) -> Self {
        Self {
            list: value.list,
            focus: value.activate,
            fullscreen: value.fullscreen,
            close: value.close,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowCommand {
    Focus(ToplevelId),
    Fullscreen { id: ToplevelId, fullscreen: bool },
    Close(ToplevelId),
}

pub fn window_entries(
    toplevels: &[ToplevelInfo],
    capabilities: WindowControlCapabilities,
) -> Vec<WindowEntry> {
    let mut entries = toplevels
        .iter()
        .map(|window| WindowEntry {
            id: window.id.to_string(),
            title: if window.title.is_empty() {
                window
                    .app_id
                    .clone()
                    .unwrap_or_else(|| "Untitled window".to_owned())
            } else {
                window.title.clone()
            },
            app_id: window.app_id.clone(),
            focused: window.state.activated,
            fullscreen: window.state.fullscreen,
            focusable: capabilities.focus,
            fullscreen_controllable: capabilities.fullscreen,
            closable: capabilities.close,
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| (!entry.focused, entry.title.clone(), entry.id.clone()));
    entries
}

pub fn parse_window_command(
    action: &MenuAction,
    windows: &[WindowEntry],
) -> Result<Option<WindowCommand>, String> {
    match action {
        MenuAction::Custom { kind, payload } if kind == "window.focus" => {
            let toplevel = parse_live_id(payload)?;
            let current = find_window(windows, payload)?;
            if !current.focusable {
                return Err("compositor does not expose toplevel activation".to_owned());
            }
            Ok(Some(WindowCommand::Focus(toplevel)))
        }
        MenuAction::Custom { kind, payload } if kind == "window.close" => {
            let toplevel = parse_live_id(payload)?;
            let current = find_window(windows, payload)?;
            if !current.closable {
                return Err("compositor does not expose toplevel close".to_owned());
            }
            Ok(Some(WindowCommand::Close(toplevel)))
        }
        MenuAction::Toggle { id } if id.starts_with("window.fullscreen:") => {
            let payload = id
                .strip_prefix("window.fullscreen:")
                .ok_or_else(|| "malformed fullscreen action".to_owned())?;
            let toplevel = parse_live_id(payload)?;
            let current = find_window(windows, payload)?;
            if !current.fullscreen_controllable {
                return Err("compositor does not expose toplevel fullscreen control".to_owned());
            }
            Ok(Some(WindowCommand::Fullscreen {
                id: toplevel,
                fullscreen: !current.fullscreen,
            }))
        }
        _ => Ok(None),
    }
}

pub fn execute_window_command(
    backend: &mut WaylandBackend,
    command: WindowCommand,
) -> Result<(), String> {
    match command {
        WindowCommand::Focus(id) => backend
            .activate_toplevel(id)
            .map_err(|error| error.to_string()),
        WindowCommand::Fullscreen { id, fullscreen } => backend
            .set_toplevel_fullscreen(id, fullscreen, None)
            .map_err(|error| error.to_string()),
        WindowCommand::Close(id) => backend
            .close_toplevel(id)
            .map_err(|error| error.to_string()),
    }
}

fn find_window<'a>(windows: &'a [WindowEntry], id: &str) -> Result<&'a WindowEntry, String> {
    windows
        .iter()
        .find(|window| window.id == id)
        .ok_or_else(|| format!("toplevel {id} is not in the current snapshot"))
}

fn parse_live_id(value: &str) -> Result<ToplevelId, String> {
    value
        .parse::<ToplevelId>()
        .map_err(|error| format!("invalid opaque toplevel id {value:?}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nuraloumi_wayland::{ToplevelSource, ToplevelState};

    fn info(id: u64, title: &str, focused: bool, fullscreen: bool) -> ToplevelInfo {
        ToplevelInfo {
            id: format!("tl:{id:016x}").parse().expect("opaque id"),
            title: title.to_owned(),
            app_id: Some("same.app".to_owned()),
            state: ToplevelState {
                activated: focused,
                fullscreen,
                ..ToplevelState::default()
            },
            outputs: Vec::new(),
            source: ToplevelSource::WlrManagement,
            protocol_identifier: None,
        }
    }

    fn full_capabilities() -> WindowControlCapabilities {
        WindowControlCapabilities {
            list: true,
            focus: true,
            fullscreen: true,
            close: true,
        }
    }

    #[test]
    fn mapping_uses_opaque_ids_even_for_duplicate_app_ids() {
        let entries = window_entries(
            &[
                info(1, "First", false, false),
                info(2, "Second", true, true),
            ],
            full_capabilities(),
        );
        assert_eq!(entries[0].id, "tl:0000000000000002");
        assert_eq!(entries[1].id, "tl:0000000000000001");
        assert_ne!(entries[0].id, entries[1].id);
        assert_eq!(entries[0].app_id.as_deref(), Some("same.app"));
        assert_eq!(entries[1].app_id.as_deref(), Some("same.app"));
        assert!(entries.iter().all(|entry| entry.focusable));
    }

    #[test]
    fn ext_list_mapping_is_read_only() {
        let entries = window_entries(
            &[info(1, "Listed", true, false)],
            WindowControlCapabilities {
                list: true,
                ..WindowControlCapabilities::default()
            },
        );
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].focusable);
        assert!(!entries[0].fullscreen_controllable);
        assert!(!entries[0].closable);
    }

    #[test]
    fn fullscreen_command_reads_current_state_by_opaque_id() {
        let windows = vec![WindowEntry {
            id: "tl:0000000000000009".to_owned(),
            title: "Terminal".to_owned(),
            app_id: Some("foot".to_owned()),
            focused: true,
            fullscreen: false,
            focusable: true,
            fullscreen_controllable: true,
            closable: true,
        }];
        let command = parse_window_command(
            &MenuAction::Toggle {
                id: "window.fullscreen:tl:0000000000000009".to_owned(),
            },
            &windows,
        )
        .expect("parse")
        .expect("window command");
        assert_eq!(
            command,
            WindowCommand::Fullscreen {
                id: "tl:0000000000000009".parse().expect("id"),
                fullscreen: true,
            }
        );
    }

    #[test]
    fn read_only_entries_fail_closed_for_control() {
        let windows = window_entries(
            &[info(7, "Read only", true, false)],
            WindowControlCapabilities {
                list: true,
                ..WindowControlCapabilities::default()
            },
        );
        assert!(parse_window_command(
            &MenuAction::Custom {
                kind: "window.focus".to_owned(),
                payload: windows[0].id.clone(),
            },
            &windows,
        )
        .is_err());
    }

    #[test]
    fn titles_and_app_ids_are_never_accepted_as_control_ids() {
        assert!(parse_window_command(
            &MenuAction::Custom {
                kind: "window.focus".to_owned(),
                payload: "foot".to_owned(),
            },
            &[],
        )
        .is_err());
        assert!(parse_window_command(
            &MenuAction::Custom {
                kind: "window.close".to_owned(),
                payload: "Terminal".to_owned(),
            },
            &[],
        )
        .is_err());
    }
}
