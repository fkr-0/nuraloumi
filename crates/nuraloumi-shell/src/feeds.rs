//! Offline feed view-model. No network or command execution while painting.
//! This temporary source import is replaced by nuraloumi_providers::feeds
//! when the integration lane publishes its module export.
#[allow(dead_code)] // Temporary accepted provider source seam until shared export lands.
#[path = "../../nuraloumi-providers/src/feeds.rs"]
pub mod provider;

use nuraloumi_core::{MenuAction, MenuItem, MenuModel, MenuState};
use nuraloumi_providers::command::{CommandLimits, CommandRunner, CommandSpec};
use provider::{
    import_opml, parse_feed, FeedEntry, OpenInBrowser, ReadState, Subscription, MAX_ENTRIES,
    MAX_SUBSCRIPTIONS,
};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

pub const PAGE_SIZE: usize = 6;

#[derive(Debug, Clone)]
pub struct Article {
    pub feed: String,
    pub feed_title: String,
    pub entry: FeedEntry,
}
pub enum Effect {
    None,
    Redraw,
    Browser(OpenInBrowser),
}
pub fn action(kind: &str, payload: impl Into<String>) -> MenuAction {
    MenuAction::Custom {
        kind: kind.into(),
        payload: payload.into(),
    }
}
fn short(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        format!(
            "{}…",
            s.chars().take(max.saturating_sub(1)).collect::<String>()
        )
    }
}

pub struct FeedView {
    pub articles: Vec<Article>,
    pub subscriptions: Vec<Subscription>,
    pub read: ReadState,
    pub selected: usize,
    pub message: String,
    state_path: Option<PathBuf>,
}
impl FeedView {
    pub fn new(
        sources: Vec<(String, Vec<u8>)>,
        opml: Option<&[u8]>,
        read: ReadState,
        state_path: Option<PathBuf>,
    ) -> Result<Self, String> {
        if sources.len() > MAX_SUBSCRIPTIONS {
            return Err("too many feeds".into());
        }
        let subscriptions = opml
            .map(import_opml)
            .transpose()
            .map_err(|e| e.to_string())?
            .unwrap_or_default();
        let mut articles = Vec::new();
        let mut seen = BTreeSet::new();
        for (feed, xml) in sources {
            OpenInBrowser::new(&feed).map_err(|e| e.to_string())?;
            let parsed = parse_feed(&xml).map_err(|e| format!("{feed}: {e}"))?;
            for entry in parsed.entries {
                if !seen.insert((feed.clone(), entry.id.clone())) {
                    continue;
                }
                if articles.len() >= MAX_ENTRIES {
                    return Err("aggregate entry cap exceeded".into());
                }
                articles.push(Article {
                    feed: feed.clone(),
                    feed_title: parsed.title.clone(),
                    entry,
                });
            }
        }
        Ok(Self {
            articles,
            subscriptions,
            read,
            selected: 0,
            message: "Offline — no network access".into(),
            state_path,
        })
    }
    pub fn fixture() -> Result<Self, String> {
        Self::new(
            vec![(
                "https://feeds.example.test/news".into(),
                include_bytes!("../../../examples/feed-fixtures/rss.xml").to_vec(),
            )],
            Some(include_bytes!("../../../examples/feed-fixtures/feeds.opml")),
            ReadState::new(),
            None,
        )
    }
    pub fn page(&self) -> usize {
        self.selected / PAGE_SIZE
    }
    pub fn pages(&self) -> usize {
        self.articles.len().max(1).div_ceil(PAGE_SIZE)
    }
    pub fn select_delta(&mut self, delta: i8) -> bool {
        match delta {
            -1 if self.selected > 0 => self.selected -= 1,
            1 if self.selected + 1 < self.articles.len() => self.selected += 1,
            _ => return false,
        }
        true
    }
    pub fn menu(&self) -> MenuModel {
        let nread = self
            .articles
            .iter()
            .filter(|a| self.read.is_read(&a.feed, &a.entry.id))
            .count();
        let mut rows = vec![MenuItem::status(
            "count",
            format!(
                "{} articles · {nread} read · {} subscriptions",
                self.articles.len(),
                self.subscriptions.len()
            ),
        )
        .with_subtitle(self.message.clone())];
        if self.articles.is_empty() {
            rows.push(MenuItem::status(
                "empty",
                "No local entries. Use --feed URL=XML_PATH.",
            ));
        } else {
            let start = self.page() * PAGE_SIZE;
            for n in start..(start + PAGE_SIZE).min(self.articles.len()) {
                let a = &self.articles[n];
                rows.push(
                    MenuItem::action(
                        format!("article-{n}"),
                        format!(
                            "{} {}",
                            if self.read.is_read(&a.feed, &a.entry.id) {
                                "✓"
                            } else {
                                "•"
                            },
                            short(&a.entry.title, 80)
                        ),
                        action("select", n.to_string()),
                    )
                    .with_subtitle(short(&a.feed_title, 70)),
                );
            }
            let a = &self.articles[self.selected];
            rows.push(
                MenuItem::status(
                    "summary",
                    short(a.entry.summary.as_deref().unwrap_or("(No summary)"), 200),
                )
                .with_subtitle(
                    a.entry
                        .published
                        .clone()
                        .unwrap_or_else(|| "Undated".into()),
                ),
            );
            rows.push(MenuItem::action(
                "mark",
                if self.read.is_read(&a.feed, &a.entry.id) {
                    "Mark unread [Enter/R]"
                } else {
                    "Mark read [Enter/R]"
                },
                action("mark", ""),
            ));
            rows.push(MenuItem::action(
                "open",
                "Open in browser [O]",
                action("open", ""),
            ));
        }
        rows.push(MenuItem::action(
            "prev",
            format!("← Previous · {}/{}", self.page() + 1, self.pages()),
            action("prev", ""),
        ));
        rows.push(MenuItem::action("next", "Next page →", action("next", "")));
        MenuModel::new("offline-feeds", "Offline feeds", rows)
    }
    pub fn menu_state(&self, model: &MenuModel) -> MenuState {
        let mut state = MenuState::new(model);
        if !self.articles.is_empty() {
            state.selected_id = Some(format!("article-{}", self.selected));
        }
        state
    }
    fn mark(&mut self) -> Result<Effect, String> {
        let a = self
            .articles
            .get(self.selected)
            .ok_or("no selected article")?;
        let (feed, id) = (a.feed.clone(), a.entry.id.clone());
        let old = self.read.clone();
        if self.read.is_read(&feed, &id) {
            self.read.mark_unread(&feed, &id);
        } else {
            self.read.mark_read(&feed, &id).map_err(|e| e.to_string())?;
        }
        if let Some(path) = &self.state_path {
            let save = (|| {
                let parent = path.parent().ok_or("read-state parent missing")?;
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                self.read.save_atomic(path).map_err(|e| e.to_string())
            })();
            if let Err(e) = save {
                self.read = old;
                return Err(e);
            }
        }
        self.message = "Updated persistent read-state".into();
        Ok(Effect::Redraw)
    }
    pub fn activate(&mut self, a: MenuAction) -> Result<Effect, String> {
        let MenuAction::Custom { kind, payload } = a else {
            return Ok(Effect::None);
        };
        match kind.as_str() {
            "select" => {
                if let Ok(n) = payload.parse::<usize>() {
                    if n < self.articles.len() && n / PAGE_SIZE == self.page() {
                        self.selected = n;
                        return Ok(Effect::Redraw);
                    }
                }
                Ok(Effect::None)
            }
            "prev" if self.page() > 0 => {
                self.selected = (self.page() - 1) * PAGE_SIZE;
                Ok(Effect::Redraw)
            }
            "next" if self.page() + 1 < self.pages() => {
                self.selected = (self.page() + 1) * PAGE_SIZE;
                Ok(Effect::Redraw)
            }
            "mark" if !self.articles.is_empty() => self.mark(),
            "open" => {
                let Some(url) = self
                    .articles
                    .get(self.selected)
                    .and_then(|a| a.entry.url.as_deref())
                else {
                    self.message = "Selected entry has no safe browser URL".into();
                    return Ok(Effect::Redraw);
                };
                Ok(Effect::Browser(
                    OpenInBrowser::new(url).map_err(|e| e.to_string())?,
                ))
            }
            _ => Ok(Effect::None),
        }
    }
}
pub fn read_bounded(path: &Path, cap: usize) -> Result<Vec<u8>, String> {
    let mut file = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut data = Vec::new();
    file.by_ref()
        .take(cap as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| e.to_string())?;
    if data.len() > cap {
        return Err(format!("{} exceeds {cap} bytes", path.display()));
    }
    Ok(data)
}
pub fn open_browser<R: CommandRunner>(runner: &mut R, req: &OpenInBrowser) -> Result<(), String> {
    let output = runner
        .run(
            &CommandSpec::new("xdg-open").arg(req.url()),
            CommandLimits::new(Duration::from_secs(3), 1024),
        )
        .map_err(|e| e.to_string())?;
    if output.status != 0 {
        return Err(format!("xdg-open exited {}", output.status));
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use nuraloumi_providers::command::{CommandOutput, FixtureCommandRunner};
    #[test]
    fn model_keyboard_and_toggling() {
        let mut view = FeedView::fixture().unwrap();
        assert_eq!(view.articles.len(), 2);
        assert_eq!(view.subscriptions.len(), 2);
        let menu = view.menu();
        menu.validate().unwrap();
        assert_eq!(
            view.menu_state(&menu).selected_id.as_deref(),
            Some("article-0")
        );
        view.activate(action("mark", "")).unwrap();
        assert!(view
            .read
            .is_read(&view.articles[0].feed, &view.articles[0].entry.id));
        view.activate(action("mark", "")).unwrap();
        assert!(!view
            .read
            .is_read(&view.articles[0].feed, &view.articles[0].entry.id));
        assert!(view.select_delta(1));
        assert!(!view.select_delta(1));
    }
    #[test]
    fn pagination_and_aggregate_limit() {
        let xml = include_bytes!("../../../examples/feed-fixtures/rss.xml").to_vec();
        let docs = (0..5)
            .map(|i| (format!("https://example.net/feed/{i}"), xml.clone()))
            .collect();
        let mut view = FeedView::new(docs, None, ReadState::new(), None).unwrap();
        assert_eq!(view.pages(), 2);
        view.activate(action("next", "")).unwrap();
        assert_eq!(view.selected, 6);
        view.activate(action("prev", "")).unwrap();
        assert_eq!(view.selected, 0);
        let docs = (0..65)
            .map(|i| (format!("https://example.net/feed/{i}"), xml.clone()))
            .collect();
        assert!(FeedView::new(docs, None, ReadState::new(), None).is_err());
    }
    #[test]
    fn exact_argv_and_malicious_url() {
        let req = OpenInBrowser::new("https://example.net/story?x=1&y=2").unwrap();
        let mut runner = FixtureCommandRunner::default();
        runner.insert(
            CommandSpec::new("xdg-open").arg(req.url()),
            CommandOutput {
                status: 0,
                stdout: String::new(),
                stderr: String::new(),
            },
        );
        open_browser(&mut runner, &req).unwrap();
        assert!(OpenInBrowser::new("javascript:alert(1)").is_err());
    }
}
