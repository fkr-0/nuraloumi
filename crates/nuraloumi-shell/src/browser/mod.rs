//! NuraLoumi-owned native browser chrome model.
//!
//! All rendering is performed by NuraLoumi's Cairo/wl_shm renderer. WPE
//! WebKit is a separate, same-UID native engine: it never draws the toolbar
//! and never accepts page-controlled JavaScript for chrome actions.

mod transport;

use nuraloumi_core::{MenuAction, MenuItem, MenuModel};
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub use transport::NativeBrowser;

pub const MAX_TABS: usize = 6;
pub const MAX_BOOKMARK_PAGE: usize = 8;
const MAX_LABEL_CHARACTERS: usize = 88;
const MAX_ADDRESS: usize = 4096;

pub trait BrowserRpc {
    fn request(&mut self, command: &str, argument: Option<&str>) -> Result<Value, String>;
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Tab {
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub loading: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct PageStatus {
    #[serde(default)]
    pub uri: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub is_loading: bool,
    #[serde(default)]
    pub can_go_back: bool,
    #[serde(default)]
    pub can_go_forward: bool,
    #[serde(default)]
    pub bookmarked: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct TabsStatus {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
pub struct BookmarksStatus {
    pub entries: Vec<Bookmark>,
    pub total: usize,
    pub offset: usize,
}

#[derive(Debug, Clone)]
pub struct BrowserUi {
    pub page: PageStatus,
    pub tabs: TabsStatus,
    pub bookmarks: BookmarksStatus,
    pub bookmark_page: usize,
    pub bookmarks_open: bool,
    pub editing_address: bool,
    pub address: String,
    pub status: String,
}

impl Default for BrowserUi {
    fn default() -> Self {
        Self {
            page: PageStatus::default(),
            tabs: TabsStatus {
                tabs: vec![Tab::default()],
                active: 0,
            },
            bookmarks: BookmarksStatus::default(),
            bookmark_page: 0,
            bookmarks_open: false,
            editing_address: false,
            address: String::new(),
            status: "WPE browser engine not connected".into(),
        }
    }
}

fn sanitize(text: &str, max: usize) -> String {
    let mut result: String = text
        .chars()
        .filter(|character| !character.is_control())
        .take(max)
        .collect();
    if text.chars().count() > max {
        result.push('…');
    }
    result
}

fn action(id: &str, label: impl Into<String>, kind: &str, payload: impl Into<String>) -> MenuItem {
    MenuItem::action(
        id,
        label.into(),
        MenuAction::Custom {
            kind: kind.into(),
            payload: payload.into(),
        },
    )
}

impl BrowserUi {
    /// A fully offline fixture for tests, UI sketches and recovery desktops.
    pub fn fixture() -> Self {
        Self {
            page: PageStatus {
                uri: "https://example.org/welcome".into(),
                title: "Welcome to NuraLoumi".into(),
                can_go_back: true,
                bookmarked: true,
                ..PageStatus::default()
            },
            tabs: TabsStatus {
                tabs: vec![
                    Tab {
                        uri: "https://example.org/welcome".into(),
                        title: "Welcome".into(),
                        loading: false,
                    },
                    Tab {
                        uri: "https://example.org/docs".into(),
                        title: "Documentation".into(),
                        loading: false,
                    },
                ],
                active: 0,
            },
            bookmarks: BookmarksStatus {
                entries: vec![
                    Bookmark {
                        url: "https://example.org/welcome".into(),
                        title: "Welcome".into(),
                    },
                    Bookmark {
                        url: "https://example.org/docs".into(),
                        title: "Documentation".into(),
                    },
                ],
                total: 2,
                offset: 0,
            },
            bookmark_page: 0,
            bookmarks_open: true,
            editing_address: false,
            address: "https://example.org/welcome".into(),
            status: "Offline UI fixture — no WPE engine or web traffic".into(),
        }
    }

    pub fn refresh(&mut self, rpc: &mut impl BrowserRpc) -> Result<(), String> {
        let page: PageStatus = serde_json::from_value(rpc.request("uri", None)?)
            .map_err(|_| "Malformed WebKit page status".to_owned())?;
        let tabs: TabsStatus = serde_json::from_value(rpc.request("tabs", None)?)
            .map_err(|_| "Malformed WebKit tabs status".to_owned())?;
        if tabs.tabs.is_empty() || tabs.tabs.len() > MAX_TABS || tabs.active >= tabs.tabs.len() {
            return Err("Invalid WebKit tab count or active tab index".into());
        }
        let page_number = self.bookmark_page.min(7);
        let bookmarks: BookmarksStatus =
            serde_json::from_value(rpc.request("bookmarks", Some(&page_number.to_string()))?)
                .map_err(|_| "Malformed bookmark state".to_owned())?;
        if bookmarks.entries.len() > MAX_BOOKMARK_PAGE
            || bookmarks.total > 64
            || bookmarks.offset != page_number * MAX_BOOKMARK_PAGE
        {
            return Err("Unbounded or inconsistent bookmark state".into());
        }
        if !self.editing_address {
            self.address = page.uri.clone();
        }
        self.page = page;
        self.tabs = tabs;
        self.bookmarks = bookmarks;
        self.status.clear();
        Ok(())
    }

    pub fn focus_address(&mut self) {
        self.editing_address = true;
        self.address = self.page.uri.clone();
    }

    pub fn text_input(&mut self, text: &str) {
        if self.editing_address
            && self.address.len().saturating_add(text.len()) <= MAX_ADDRESS
            && !text.chars().any(char::is_control)
        {
            self.address.push_str(text);
        }
    }

    pub fn backspace(&mut self) {
        if self.editing_address {
            self.address.pop();
        }
    }

    pub fn cancel_address(&mut self) {
        self.editing_address = false;
        self.address = self.page.uri.clone();
    }

    fn command(
        &mut self,
        rpc: &mut impl BrowserRpc,
        cmd: &str,
        arg: Option<&str>,
    ) -> Result<(), String> {
        rpc.request(cmd, arg)?;
        self.refresh(rpc)
    }

    pub fn submit_address(&mut self, rpc: &mut impl BrowserRpc) -> Result<(), String> {
        let entered = self.address.trim();
        let candidate = if entered.starts_with("http://") || entered.starts_with("https://") {
            entered.to_owned()
        } else {
            format!("https://{entered}")
        };
        if candidate.len() > MAX_ADDRESS
            || candidate.chars().any(char::is_control)
            || !(candidate.starts_with("http://") || candidate.starts_with("https://"))
        {
            return Err("Address must be an absolute HTTP(S) URL".into());
        }
        self.command(rpc, "open", Some(&candidate))?;
        self.editing_address = false;
        Ok(())
    }

    pub fn activate(
        &mut self,
        action: MenuAction,
        rpc: &mut impl BrowserRpc,
    ) -> Result<(), String> {
        let MenuAction::Custom { kind, payload } = action else {
            return Ok(());
        };
        let result = match kind.as_str() {
            "browser.address" => {
                self.focus_address();
                Ok(())
            }
            "browser.go" => return self.submit_address(rpc),
            "browser.back" => self.command(rpc, "back", None),
            "browser.forward" => self.command(rpc, "forward", None),
            "browser.reload" => self.command(rpc, "reload", None),
            "browser.stop" => self.command(rpc, "stop", None),
            "browser.tab.new" => self.command(rpc, "tab-new", None),
            "browser.tab.select" => self.command(rpc, "tab-select", Some(&payload)),
            "browser.tab.close" => self.command(rpc, "tab-close", Some(&payload)),
            "browser.bookmark.toggle" => self.command(rpc, "bookmark-toggle", None),
            "browser.bookmark.open" => {
                self.bookmarks_open = true;
                self.refresh(rpc)
            }
            "browser.bookmark.hide" => {
                self.bookmarks_open = false;
                Ok(())
            }
            "browser.bookmark.goto" => self.command(rpc, "open", Some(&payload)),
            "browser.bookmark.page" => {
                self.bookmark_page = payload.parse::<usize>().unwrap_or(0).min(7);
                self.refresh(rpc)
            }
            _ => Ok(()),
        };
        if let Err(message) = &result {
            self.status = message.clone();
        }
        result
    }

    pub fn menu(&self) -> MenuModel {
        let mut items = Vec::new();
        // Dedicated tab strip in the Cairo layout, fixed outside scrolling.
        for (index, tab) in self.tabs.tabs.iter().take(MAX_TABS).enumerate() {
            let label = if tab.title.trim().is_empty() {
                if tab.uri.is_empty() || tab.uri == "about:blank" {
                    format!("Tab {}", index + 1)
                } else {
                    sanitize(&tab.uri, 16)
                }
            } else {
                sanitize(&tab.title, 16)
            };
            let mut item = action(
                &format!("browser.tab.{index}"),
                label,
                "browser.tab.select",
                index.to_string(),
            );
            if index == self.tabs.active {
                item.enabled = false; // renderer highlights active tab
            }
            items.push(item);
        }

        items.push(MenuItem::status("browser.address.header", "Address bar"));
        let address = sanitize(&self.address, MAX_LABEL_CHARACTERS);
        items.push(
            action(
                "browser.address",
                format!(
                    "{} {}",
                    if self.editing_address { "✎" } else { "⌕" },
                    address
                ),
                "browser.address",
                "",
            )
            .with_subtitle(if self.editing_address {
                "Typing address · Enter to open · Escape to cancel"
            } else {
                "Tap to edit · keyboard input supported"
            }),
        );
        items.push(action("browser.go", "↗ Go to address", "browser.go", ""));
        items.push(MenuItem::separator("browser.nav.separator"));

        for (id, label, kind, enabled) in [
            (
                "browser.back",
                "← Back",
                "browser.back",
                self.page.can_go_back,
            ),
            (
                "browser.forward",
                "→ Forward",
                "browser.forward",
                self.page.can_go_forward,
            ),
            (
                "browser.reload",
                "⟳ Reload",
                "browser.reload",
                !self.page.is_loading,
            ),
            (
                "browser.stop",
                "■ Stop",
                "browser.stop",
                self.page.is_loading,
            ),
            (
                "browser.tab.new",
                "+ New tab",
                "browser.tab.new",
                self.tabs.tabs.len() < MAX_TABS,
            ),
            (
                "browser.tab.close",
                "× Close current tab",
                "browser.tab.close",
                self.tabs.tabs.len() > 1,
            ),
        ] {
            let mut item = action(id, label, kind, self.tabs.active.to_string());
            item.enabled = enabled;
            items.push(item);
        }
        items.push(action(
            "browser.bookmark.toggle",
            if self.page.bookmarked {
                "★ Remove bookmark"
            } else {
                "☆ Save bookmark"
            },
            "browser.bookmark.toggle",
            "",
        ));
        items.push(action(
            "browser.bookmark.expand",
            if self.bookmarks_open {
                "▴ Hide bookmarks"
            } else {
                "▾ Bookmarks"
            },
            if self.bookmarks_open {
                "browser.bookmark.hide"
            } else {
                "browser.bookmark.open"
            },
            "",
        ));
        if self.bookmarks_open {
            items.push(MenuItem::status(
                "browser.bookmark.header",
                format!("Saved pages · {} total", self.bookmarks.total),
            ));
            for (index, bookmark) in self.bookmarks.entries.iter().enumerate() {
                items.push(
                    action(
                        &format!("browser.bookmark.{index}"),
                        format!("★ {}", sanitize(&bookmark.title, 48)),
                        "browser.bookmark.goto",
                        bookmark.url.clone(),
                    )
                    .with_subtitle(sanitize(&bookmark.url, 72)),
                );
            }
            if self.bookmark_page > 0 {
                items.push(action(
                    "browser.bookmark.previous",
                    "← Earlier bookmarks",
                    "browser.bookmark.page",
                    (self.bookmark_page - 1).to_string(),
                ));
            }
            if (self.bookmark_page + 1) * MAX_BOOKMARK_PAGE < self.bookmarks.total {
                items.push(action(
                    "browser.bookmark.next",
                    "More bookmarks →",
                    "browser.bookmark.page",
                    (self.bookmark_page + 1).to_string(),
                ));
            }
        }
        if !self.status.is_empty() {
            items.push(MenuItem::status(
                "browser.status",
                sanitize(&self.status, 96),
            ));
        }
        MenuModel::new("browser", "Nura Browser", items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRpc;
    impl BrowserRpc for FakeRpc {
        fn request(&mut self, cmd: &str, argument: Option<&str>) -> Result<Value, String> {
            match cmd {
                "uri" => Ok(serde_json::json!({
                    "uri": "https://example.org/", "title": "Example",
                    "can_go_back": true, "bookmarked": true
                })),
                "tabs" => Ok(serde_json::json!({
                    "active": 0, "tabs": [{"uri":"https://example.org/","title":"Example"}]
                })),
                "bookmarks" => Ok(serde_json::json!({
                    "offset":argument.unwrap_or("0").parse::<usize>().unwrap()*8,
                    "total":1, "entries":[{"url":"https://example.org/","title":"Example"}]
                })),
                "open" | "tab-new" | "tab-select" | "tab-close" | "bookmark-toggle" | "back"
                | "forward" | "reload" | "stop" => Ok(Value::Null),
                _ => Err("unexpected browser command".into()),
            }
        }
    }

    #[test]
    fn fixture_has_real_menu_actions_and_horizontal_tabs() {
        let ui = BrowserUi::fixture();
        let menu = ui.menu();
        assert!(menu.items[0].id.starts_with("browser.tab."));
        assert!(!menu.items[0].enabled);
        assert!(menu.items[1].enabled);
        assert!(menu.item_by_id("browser.address").is_some());
        assert!(menu.item_by_id("browser.bookmark.0").is_some());
        assert!(menu.item_by_id("browser.tab.close").is_some());
        nuraloumi_core::validate_menu(&menu).unwrap();
    }

    #[test]
    fn address_editor_is_bounded_and_does_not_clobber_on_refresh() {
        let mut ui = BrowserUi::fixture();
        ui.focus_address();
        ui.address.clear();
        ui.text_input("example.net/path");
        ui.text_input("\n");
        assert_eq!(ui.address, "example.net/path");
        ui.backspace();
        assert_eq!(ui.address, "example.net/pat");
        ui.refresh(&mut FakeRpc).unwrap();
        assert_eq!(ui.address, "example.net/pat");
        ui.submit_address(&mut FakeRpc).unwrap();
        assert!(!ui.editing_address);
        assert_eq!(ui.page.uri, "https://example.org/");
    }

    #[test]
    fn bookmark_paging_and_snapshot_bounds_are_checked() {
        let mut ui = BrowserUi {
            bookmark_page: 1,
            ..BrowserUi::default()
        };
        ui.refresh(&mut FakeRpc).unwrap();
        assert_eq!(ui.bookmarks.offset, 8);
        assert!(ui.tabs.tabs.len() <= MAX_TABS);
        ui.activate(
            MenuAction::Custom {
                kind: "browser.bookmark.page".into(),
                payload: "3".into(),
            },
            &mut FakeRpc,
        )
        .unwrap();
        assert_eq!(ui.bookmark_page, 3);
    }
}
