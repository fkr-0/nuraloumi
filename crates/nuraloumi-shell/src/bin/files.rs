//! Native, rooted, paginated file manager (Cairo / wl_shm, no EGL).
#[path = "../file_manager.rs"]
mod file_manager;

use file_manager::{valid_name, Browser, Capabilities, Entry};
use nuraloumi_core::{MenuAction, MenuItem, MenuModel, MenuState, DARK_THEME, LIGHT_THEME};
use nuraloumi_providers::file_open::{self, DesktopHandler, OpenOutcome};
use nuraloumi_render_cairo::{CairoRenderer, RenderOptions, Scene, Viewport};
use nuraloumi_shell::{load_config_or_default, render_menu_follow_selection, ShellConfig, Theme};
use nuraloumi_wayland::{
    BackendError, Frame, Key, MenuConfig, PixelFormat, PlatformEvent, SurfaceId, WaylandBackend,
};
#[cfg(unix)]
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::{env, thread};

// Four rows leave room for touch-accessible navigation and action controls
// on the SL101 640-logical-pixel menu (minimum 48px action targets).
const PAGE_ROWS: usize = 4;
const MAX_CHOOSER_APPS: usize = 64;
const MAX_CHILDREN: usize = 4;
static LIVE_CHILDREN: AtomicUsize = AtomicUsize::new(0);

const HELP: &str = "nuraloumi-files [--live] [--config PATH] [--allow-rename] [--allow-delete] [--open-with DESKTOP_ID] [ROOT]\n\
Read-only by default. Arrows / J,K select, Enter/Right opens, Left/Backspace/Esc goes to parent, [ ] pages, O Open With, R rename, D delete, U refresh.\n\
Touch/pointer rows activate on release-inside. Rename/delete require explicit opt-in and confirmation; delete is non-recursive.\n";

#[derive(Debug)]
struct Options {
    live: bool,
    config: Option<PathBuf>,
    root: PathBuf,
    capabilities: Capabilities,
    open_with: Option<String>,
}
fn parse<I: IntoIterator<Item = String>>(args: I) -> Result<Option<Options>, String> {
    let mut out = Options {
        live: false,
        config: None,
        root: env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/")),
        capabilities: Capabilities::default(),
        open_with: None,
    };
    let mut iter = args.into_iter();
    let mut root_seen = false;
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{HELP}");
                return Ok(None);
            }
            "--live" => out.live = true,
            "--config" => out.config = Some(iter.next().ok_or("--config needs PATH")?.into()),
            "--allow-rename" => out.capabilities.rename = true,
            "--allow-delete" => out.capabilities.delete = true,
            "--open-with" => out.open_with = Some(iter.next().ok_or("--open-with needs ID")?),
            other if !other.starts_with('-') && !root_seen => {
                out.root = PathBuf::from(other);
                root_seen = true;
            }
            _ => return Err(format!("unknown or repeated argument {arg}")),
        }
    }
    if out
        .open_with
        .as_deref()
        .is_some_and(|id| !file_manager::valid_desktop_id(id))
    {
        return Err("unsafe .desktop ID".into());
    }
    Ok(Some(out))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    Normal,
    RenameInput {
        index: usize,
        expected: Entry,
        name: String,
    },
    RenameConfirm {
        index: usize,
        expected: Entry,
        name: String,
    },
    DeleteConfirm {
        index: usize,
        expected: Entry,
    },
    OpenWith {
        index: usize,
        expected: Entry,
        reason: String,
        handlers: Vec<DesktopHandler>,
        selected: usize,
    },
    Fallback {
        index: usize,
        expected: Entry,
        reason: String,
        detail: String,
    },
    Busy {
        index: usize,
        expected: Entry,
    },
}

// The Wayland thread never waits on MIME probes, application scanning or
// helper process completion. One queued job + one response per surface.
enum JobKind {
    Default,
    Handlers { reason: String },
    Launch { id: String },
    Describe { reason: String },
}
struct Job {
    index: usize,
    expected: Entry,
    path: PathBuf,
    kind: JobKind,
}
enum ReplyKind {
    Default(OpenOutcome),
    Handlers {
        reason: String,
        handlers: Vec<DesktopHandler>,
    },
    Launch(Result<String, String>),
    Describe {
        reason: String,
        result: Result<String, String>,
    },
    Invalid(String),
}
struct Reply {
    index: usize,
    expected: Entry,
    kind: ReplyKind,
}
struct OpenWorker {
    requests: SyncSender<Job>,
    replies: Receiver<Reply>,
}
impl OpenWorker {
    fn new(mut ops: Box<dyn FileOps>) -> Self {
        let (requests, queue) = mpsc::sync_channel(1);
        let (sender, replies) = mpsc::sync_channel(1);
        thread::spawn(move || {
            while let Ok(job) = queue.recv() {
                let reply = perform_job(ops.as_mut(), job);
                if sender.send(reply).is_err() {
                    break;
                }
            }
        });
        Self { requests, replies }
    }
}
fn perform_job(ops: &mut dyn FileOps, job: Job) -> Reply {
    let Job {
        index,
        expected,
        path,
        kind,
    } = job;
    let verified = std::fs::symlink_metadata(&path)
        .map_err(|e| format!("file changed before opening: {e}"))
        .and_then(|meta| {
            if meta.file_type().is_symlink() || !meta.is_file() || expected.directory {
                return Err("selected entry is no longer a regular file".into());
            }
            #[cfg(unix)]
            if meta.dev() != expected.device || meta.ino() != expected.inode {
                return Err("selected file was replaced; please refresh".into());
            }
            Ok(())
        });
    let kind = match verified {
        Err(error) => ReplyKind::Invalid(error),
        Ok(()) => match kind {
            JobKind::Default => ReplyKind::Default(ops.open_default(&path)),
            JobKind::Handlers { reason } => ReplyKind::Handlers {
                reason,
                handlers: ops.handlers(&path),
            },
            JobKind::Launch { id } => ReplyKind::Launch(ops.open_with(&path, &id)),
            JobKind::Describe { reason } => ReplyKind::Describe {
                reason,
                result: ops.describe(&path),
            },
        },
    };
    Reply {
        index,
        expected,
        kind,
    }
}
trait FileOps: Send {
    fn open_default(&mut self, path: &Path) -> OpenOutcome;
    fn handlers(&mut self, path: &Path) -> Vec<DesktopHandler>;
    fn open_with(&mut self, path: &Path, id: &str) -> Result<String, String>;
    fn describe(&mut self, path: &Path) -> Result<String, String>;
}
struct SystemFiles;
impl FileOps for SystemFiles {
    fn open_default(&mut self, path: &Path) -> OpenOutcome {
        file_open::open_default(path)
    }
    fn handlers(&mut self, path: &Path) -> Vec<DesktopHandler> {
        file_open::handlers_for_path(path)
    }
    fn open_with(&mut self, path: &Path, id: &str) -> Result<String, String> {
        file_open::open_with(path, id).map(|r| r.handler)
    }
    fn describe(&mut self, path: &Path) -> Result<String, String> {
        file_open::describe_file(path)
    }
}
struct Ui {
    browser: Browser,
    selected: Option<usize>,
    page: usize,
    mode: Mode,
    pinned_app: Option<String>,
    ops: Option<Box<dyn FileOps>>,
    worker: Option<OpenWorker>,
}
fn action(kind: &str, payload: impl Into<String>) -> MenuAction {
    MenuAction::Custom {
        kind: kind.into(),
        payload: payload.into(),
    }
}
impl Ui {
    fn new(options: &Options) -> Result<Self, String> {
        let mut ui = Self::with_ops(options, Box::new(SystemFiles))?;
        ui.worker = Some(OpenWorker::new(
            ui.ops.take().ok_or("missing file backend")?,
        ));
        Ok(ui)
    }
    // Fixture mode is deliberately synchronous/deterministic; live mode uses
    // the same FileOps trait on a bounded dedicated worker.
    fn with_ops(options: &Options, ops: Box<dyn FileOps>) -> Result<Self, String> {
        let browser = Browser::new(&options.root, options.capabilities)?;
        Ok(Self {
            selected: (!browser.entries.is_empty()).then_some(0),
            browser,
            page: 0,
            mode: Mode::Normal,
            pinned_app: options.open_with.clone(),
            ops: Some(ops),
            worker: None,
        })
    }
    fn selected_entry(&self) -> Option<&Entry> {
        self.selected.and_then(|i| self.browser.entries.get(i))
    }
    fn selected_snapshot(&self) -> Result<(usize, Entry), String> {
        let index = self.selected.ok_or("no selected entry")?;
        let entry = self
            .browser
            .entries
            .get(index)
            .ok_or("selected entry disappeared")?
            .clone();
        Ok((index, entry))
    }
    fn checked_file(&self, index: usize, expected: &Entry) -> Result<PathBuf, String> {
        if self.browser.entries.get(index) != Some(expected) {
            return Err("selection changed; action cancelled".into());
        }
        self.browser.regular_file(index)
    }
    fn page_count(size: usize) -> usize {
        size.saturating_sub(1) / PAGE_ROWS + 1
    }
    fn page_label(&self) -> String {
        format!(
            "Page {}/{} | {} visible{}",
            self.page + 1,
            Self::page_count(self.browser.entries.len()),
            self.browser.entries.len(),
            if self.browser.has_more {
                " (partial directory)"
            } else {
                ""
            }
        )
    }
    fn menu(&self) -> MenuModel {
        let mut items =
            vec![
                MenuItem::status("location", self.browser.current.display().to_string())
                    .with_subtitle(self.browser.message.clone()),
            ];
        match &self.mode {
            Mode::Normal => {
                items.push(MenuItem::action(
                    "parent",
                    "..  Parent / Back",
                    action("parent", ""),
                ));
                items.push(MenuItem::status("page-info", self.page_label()));
                if self.page > 0 {
                    items.push(MenuItem::action(
                        "prev",
                        "◀ Previous page",
                        action("page-prev", ""),
                    ));
                }
                let start = self.page * PAGE_ROWS;
                for (idx, e) in self
                    .browser
                    .entries
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(PAGE_ROWS)
                {
                    let marker = if self.selected == Some(idx) {
                        "▸ "
                    } else {
                        "  "
                    };
                    let name = format!("{marker}{}{}", e.name, if e.directory { "/" } else { "" });
                    let desc = if e.directory {
                        "Directory".to_string()
                    } else {
                        format!("Regular file · {} bytes", e.len)
                    };
                    items.push(
                        MenuItem::action(
                            format!("entry-{idx}"),
                            name,
                            action("select", idx.to_string()),
                        )
                        .with_subtitle(desc),
                    );
                }
                if self.browser.entries.is_empty() {
                    items.push(MenuItem::status(
                        "empty",
                        "Empty directory / no safe visible files",
                    ));
                }
                if (self.page + 1) * PAGE_ROWS < self.browser.entries.len() {
                    items.push(MenuItem::action(
                        "next",
                        "Next page ▶",
                        action("page-next", ""),
                    ));
                }
                let selected = self
                    .selected_entry()
                    .map(|e| {
                        format!(
                            "{} | {} | {} bytes",
                            e.name,
                            if e.directory { "directory" } else { "file" },
                            e.len
                        )
                    })
                    .unwrap_or_else(|| "No entry selected".into());
                items.push(MenuItem::status(
                    "selection",
                    format!("Selected: {selected}"),
                ));
                items.push(MenuItem::action("open", "Enter / Open", action("open", "")));
                items.push(MenuItem::action(
                    "open-with",
                    "O / Open With…",
                    action("open-with", ""),
                ));
                if self.browser.capabilities.rename {
                    items.push(MenuItem::action(
                        "rename",
                        "R / Rename…",
                        action("rename", ""),
                    ));
                }
                if self.browser.capabilities.delete {
                    items.push(MenuItem::action(
                        "delete",
                        "D / Delete…",
                        action("delete", ""),
                    ));
                }
                items.push(MenuItem::action(
                    "refresh",
                    "U / Refresh",
                    action("refresh", ""),
                ));
            }
            Mode::RenameInput { expected, name, .. } => {
                items.push(
                    MenuItem::status("input", format!("Rename {} → {name}_", expected.name))
                        .with_subtitle("Type new name; Backspace edits, Enter continues"),
                );
                items.push(MenuItem::action(
                    "rename-next",
                    "Continue to confirmation",
                    action("rename-next", ""),
                ));
                items.push(MenuItem::action("cancel", "Cancel", action("cancel", "")));
            }
            Mode::RenameConfirm { expected, name, .. } => {
                items.push(MenuItem::status(
                    "warning",
                    format!("Rename {} → {name}?", expected.name),
                ));
                items.push(MenuItem::action(
                    "confirm",
                    "Confirm rename",
                    action("confirm", ""),
                ));
                items.push(MenuItem::action("cancel", "Cancel", action("cancel", "")));
            }
            Mode::DeleteConfirm { expected, .. } => {
                items.push(MenuItem::status(
                    "warning",
                    format!(
                        "Delete {}? Irreversible; directories must be empty.",
                        expected.name
                    ),
                ));
                items.push(MenuItem::action(
                    "confirm",
                    "Confirm deletion",
                    action("confirm", ""),
                ));
                items.push(MenuItem::action("cancel", "Cancel", action("cancel", "")));
            }
            Mode::OpenWith {
                expected,
                reason,
                handlers,
                selected,
                ..
            } => {
                items.push(
                    MenuItem::status("chooser-title", format!("Open With: {}", expected.name))
                        .with_subtitle(reason.clone()),
                );
                items.push(MenuItem::status(
                    "chooser-page",
                    format!(
                        "Application {}/{}",
                        if handlers.is_empty() { 0 } else { selected + 1 },
                        handlers.len()
                    ),
                ));
                let start = selected / PAGE_ROWS * PAGE_ROWS;
                for (idx, handler) in handlers.iter().enumerate().skip(start).take(PAGE_ROWS) {
                    let marker = if *selected == idx { "▸ " } else { "  " };
                    items.push(
                        MenuItem::action(
                            format!("app-{idx}"),
                            format!("{marker}{}", handler.name),
                            action("choose", idx.to_string()),
                        )
                        .with_subtitle(handler.id.clone()),
                    );
                }
                if handlers.is_empty() {
                    items.push(MenuItem::status(
                        "no-handlers",
                        "No eligible desktop application discovered",
                    ));
                }
                if start > 0 {
                    items.push(MenuItem::action(
                        "app-prev",
                        "◀ Previous applications",
                        action("app-prev", ""),
                    ));
                }
                if start + PAGE_ROWS < handlers.len() {
                    items.push(MenuItem::action(
                        "app-next",
                        "Next applications ▶",
                        action("app-next", ""),
                    ));
                }
                items.push(MenuItem::action(
                    "fallback",
                    "Diagnostics / Other options…",
                    action("fallback", ""),
                ));
                items.push(MenuItem::action("cancel", "Cancel", action("cancel", "")));
            }
            Mode::Busy { expected, .. } => {
                items.push(
                    MenuItem::status("opening", format!("Resolving {}…", expected.name))
                        .with_subtitle("Waiting for bounded opener / application discovery"),
                );
            }
            Mode::Fallback {
                expected,
                reason,
                detail,
                ..
            } => {
                items.push(
                    MenuItem::status("fallback-title", format!("No handler: {}", expected.name))
                        .with_subtitle(reason.clone()),
                );
                items.push(MenuItem::status(
                    "diagnostic",
                    format!("file -- PATH: {detail}"),
                ));
                items.push(MenuItem::action(
                    "viewer",
                    "Open generic viewer",
                    action("viewer", ""),
                ));
                items.push(MenuItem::action(
                    "terminal",
                    "Open terminal: file -- PATH",
                    action("terminal", ""),
                ));
                items.push(MenuItem::action(
                    "chooser",
                    "Try Open With…",
                    action("open-with", ""),
                ));
                items.push(MenuItem::action("cancel", "Cancel", action("cancel", "")));
            }
        }
        MenuModel::new("files", "NuraLoumi Files", items)
    }
    fn state(&self, menu: &MenuModel) -> MenuState {
        let mut state = MenuState::new(menu);
        let id = match &self.mode {
            Mode::Normal => self.selected.map(|index| format!("entry-{index}")),
            Mode::OpenWith {
                selected, handlers, ..
            } if !handlers.is_empty() => Some(format!("app-{selected}")),
            Mode::RenameInput { .. } => Some("rename-next".into()),
            Mode::RenameConfirm { .. } | Mode::DeleteConfirm { .. } => Some("confirm".into()),
            Mode::Fallback { .. } => Some("viewer".into()),
            Mode::Busy { .. } => None,
            _ => None,
        };
        if let Some(id) = id {
            if menu
                .item_by_id(&id)
                .is_some_and(|item| item.action.is_some())
            {
                state.selected_id = Some(id);
            }
        }
        state
    }
    fn change_selection(&mut self, delta: isize) {
        match &mut self.mode {
            Mode::Normal if !self.browser.entries.is_empty() => {
                let old = self.selected.unwrap_or(0);
                let index = if delta < 0 {
                    old.saturating_sub(delta.unsigned_abs())
                } else {
                    old.saturating_add(delta as usize)
                        .min(self.browser.entries.len() - 1)
                };
                self.selected = Some(index);
                self.page = index / PAGE_ROWS;
            }
            Mode::OpenWith {
                selected, handlers, ..
            } if !handlers.is_empty() => {
                let old = *selected;
                *selected = if delta < 0 {
                    old.saturating_sub(delta.unsigned_abs())
                } else {
                    old.saturating_add(delta as usize).min(handlers.len() - 1)
                };
            }
            _ => {}
        }
    }
    fn change_page(&mut self, delta: isize) {
        match &mut self.mode {
            Mode::Normal => {
                let max_page = Self::page_count(self.browser.entries.len()).saturating_sub(1);
                let next = if delta < 0 {
                    self.page.saturating_sub(delta.unsigned_abs())
                } else {
                    self.page.saturating_add(delta as usize).min(max_page)
                };
                self.page = next;
                let idx = next * PAGE_ROWS;
                self.selected = self.browser.entries.get(idx).map(|_| idx);
            }
            Mode::OpenWith {
                selected, handlers, ..
            } if !handlers.is_empty() => {
                let max_page = Self::page_count(handlers.len()).saturating_sub(1);
                let page = *selected / PAGE_ROWS;
                let next = if delta < 0 {
                    page.saturating_sub(delta.unsigned_abs())
                } else {
                    page.saturating_add(delta as usize).min(max_page)
                };
                *selected = (next * PAGE_ROWS).min(handlers.len() - 1);
            }
            _ => {}
        }
    }
    fn refresh(&mut self) -> Result<(), String> {
        let previous = self.selected_entry().cloned();
        self.browser.refresh()?;
        self.selected = previous
            .as_ref()
            .and_then(|e| {
                self.browser
                    .entries
                    .iter()
                    .position(|candidate| candidate == e)
            })
            .or_else(|| (!self.browser.entries.is_empty()).then_some(0));
        self.page = self.selected.unwrap_or(0) / PAGE_ROWS;
        self.mode = Mode::Normal;
        Ok(())
    }
    fn busy(&self) -> bool {
        matches!(self.mode, Mode::Busy { .. })
    }
    fn submit(&mut self, job: Job) -> Result<(), String> {
        let index = job.index;
        let expected = job.expected.clone();
        if let Some(worker) = &self.worker {
            worker
                .requests
                .try_send(job)
                .map_err(|e| format!("opener busy: {e}"))?;
            self.mode = Mode::Busy { index, expected };
            self.browser.message = "Resolving handler without blocking Wayland…".into();
            Ok(())
        } else {
            // Deterministic fixture mode only, never the live Wayland eventloop.
            let reply = perform_job(self.ops.as_mut().ok_or("opener missing")?.as_mut(), job);
            self.apply_reply(reply)
        }
    }
    fn apply_reply(&mut self, reply: Reply) -> Result<(), String> {
        let Reply {
            index,
            expected,
            kind,
        } = reply;
        if self.browser.entries.get(index) != Some(&expected)
            || self.browser.regular_file(index).is_err()
        {
            self.mode = Mode::Normal;
            return Err("file changed while opening; action discarded".into());
        }
        self.mode = Mode::Normal;
        match kind {
            ReplyKind::Invalid(reason) => Err(reason),
            ReplyKind::Default(OpenOutcome::Opened { handler }) => {
                self.browser.message = format!("Launch requested: {handler}");
                Ok(())
            }
            ReplyKind::Default(OpenOutcome::NeedsOpenWith { reason, handlers }) => {
                self.mode = Mode::OpenWith {
                    index,
                    expected,
                    reason,
                    handlers: handlers.into_iter().take(MAX_CHOOSER_APPS).collect(),
                    selected: 0,
                };
                Ok(())
            }
            ReplyKind::Default(OpenOutcome::NeedsFallback { reason }) => {
                self.open_chooser(index, expected, reason)
            }
            ReplyKind::Handlers { reason, handlers } => {
                self.mode = Mode::OpenWith {
                    index,
                    expected,
                    reason,
                    handlers: handlers.into_iter().take(MAX_CHOOSER_APPS).collect(),
                    selected: 0,
                };
                Ok(())
            }
            ReplyKind::Launch(result) => match result {
                Ok(handler) => {
                    self.browser.message = format!("Launch requested: {handler}");
                    Ok(())
                }
                Err(reason) => self.show_fallback(index, expected, reason),
            },
            ReplyKind::Describe { reason, result } => {
                let detail = result.unwrap_or_else(|e| format!("unavailable: {e}"));
                self.mode = Mode::Fallback {
                    index,
                    expected,
                    reason,
                    detail,
                };
                Ok(())
            }
        }
    }
    fn poll_worker(&mut self) -> bool {
        use mpsc::TryRecvError;
        let result = self.worker.as_ref().map(|worker| worker.replies.try_recv());
        match result {
            Some(Ok(reply)) if self.busy() => {
                if let Err(error) = self.apply_reply(reply) {
                    self.browser.message = format!("Action refused: {error}");
                }
                true
            }
            Some(Err(TryRecvError::Disconnected)) if self.busy() => {
                self.mode = Mode::Normal;
                self.browser.message = "Opener worker terminated unexpectedly".into();
                true
            }
            _ => false,
        }
    }
    fn enter_selected(&mut self) -> Result<(), String> {
        let Some(index) = self.selected else {
            return Ok(());
        };
        if self.browser.enter(index)? {
            self.selected = (!self.browser.entries.is_empty()).then_some(0);
            self.page = 0;
            return Ok(());
        }
        let (index, expected) = self.selected_snapshot()?;
        let path = self.checked_file(index, &expected)?;
        let kind = match self.pinned_app.clone() {
            Some(id) => JobKind::Launch { id },
            None => JobKind::Default,
        };
        self.submit(Job {
            index,
            expected,
            path,
            kind,
        })
    }
    fn open_chooser(
        &mut self,
        index: usize,
        expected: Entry,
        reason: String,
    ) -> Result<(), String> {
        let path = self.checked_file(index, &expected)?;
        self.submit(Job {
            index,
            expected,
            path,
            kind: JobKind::Handlers { reason },
        })
    }
    fn show_open_with(&mut self) -> Result<(), String> {
        let (index, expected) = match &self.mode {
            Mode::Fallback {
                index, expected, ..
            } => (*index, expected.clone()),
            _ => self.selected_snapshot()?,
        };
        self.open_chooser(
            index,
            expected,
            "Choose an eligible desktop application".into(),
        )
    }
    fn choose(&mut self, chosen: usize) -> Result<(), String> {
        let Mode::OpenWith {
            index,
            expected,
            handlers,
            ..
        } = self.mode.clone()
        else {
            return Ok(());
        };
        let app = handlers
            .get(chosen)
            .ok_or("application index out of bounds")?;
        let path = self.checked_file(index, &expected)?;
        self.submit(Job {
            index,
            expected,
            path,
            kind: JobKind::Launch { id: app.id.clone() },
        })
    }
    fn show_fallback(
        &mut self,
        index: usize,
        expected: Entry,
        reason: String,
    ) -> Result<(), String> {
        let path = self.checked_file(index, &expected)?;
        self.submit(Job {
            index,
            expected,
            path,
            kind: JobKind::Describe { reason },
        })
    }
    fn fallback(&mut self) -> Result<(), String> {
        let Mode::OpenWith {
            index,
            expected,
            reason,
            ..
        } = self.mode.clone()
        else {
            return Ok(());
        };
        self.show_fallback(index, expected, reason)
    }
    fn launch_fallback(&mut self, terminal: bool) -> Result<(), String> {
        let Mode::Fallback {
            index, expected, ..
        } = self.mode.clone()
        else {
            return Ok(());
        };
        let path = self.checked_file(index, &expected)?;
        if terminal {
            launch_argv(
                "foot",
                &[
                    std::ffi::OsStr::new("-e"),
                    std::ffi::OsStr::new("file"),
                    std::ffi::OsStr::new("--"),
                    path.as_os_str(),
                ],
            )?;
            self.browser.message = format!("Terminal file inspection requested: {}", expected.name);
        } else {
            launch_argv(
                "nuraloumi-viewer",
                &[std::ffi::OsStr::new("--live"), path.as_os_str()],
            )?;
            self.browser.message = format!("Generic viewer requested: {}", expected.name);
        }
        self.mode = Mode::Normal;
        Ok(())
    }
    fn begin_rename(&mut self) -> Result<(), String> {
        if !self.browser.capabilities.rename {
            return Err("rename disabled; use --allow-rename".into());
        }
        let (index, expected) = self.selected_snapshot()?;
        self.mode = Mode::RenameInput {
            index,
            expected,
            name: String::new(),
        };
        Ok(())
    }
    fn begin_delete(&mut self) -> Result<(), String> {
        if !self.browser.capabilities.delete {
            return Err("delete disabled; use --allow-delete".into());
        }
        let (index, expected) = self.selected_snapshot()?;
        self.mode = Mode::DeleteConfirm { index, expected };
        Ok(())
    }
    fn confirm_name(&mut self) -> Result<(), String> {
        if let Mode::RenameInput {
            index,
            expected,
            name,
        } = &self.mode
        {
            if !valid_name(name) {
                return Err("invalid new filename".into());
            }
            self.mode = Mode::RenameConfirm {
                index: *index,
                expected: expected.clone(),
                name: name.clone(),
            };
        }
        Ok(())
    }
    fn confirm(&mut self) -> Result<(), String> {
        match self.mode.clone() {
            Mode::RenameConfirm {
                index,
                expected,
                name,
            } => {
                if self.browser.entries.get(index) != Some(&expected) {
                    return Err("rename selection changed: confirmation invalidated".into());
                }
                self.browser.rename(index, &name, true)?;
            }
            Mode::DeleteConfirm { index, expected } => {
                if self.browser.entries.get(index) != Some(&expected) {
                    return Err("delete selection changed: confirmation invalidated".into());
                }
                self.browser.delete(index, &expected.name, true)?;
            }
            _ => return Ok(()),
        }
        self.mode = Mode::Normal;
        self.selected = (!self.browser.entries.is_empty()).then_some(
            self.selected
                .unwrap_or(0)
                .min(self.browser.entries.len().saturating_sub(1)),
        );
        self.page = self.selected.unwrap_or(0) / PAGE_ROWS;
        Ok(())
    }
    fn activate(&mut self, requested: MenuAction) -> Result<(), String> {
        let MenuAction::Custom { kind, payload } = requested else {
            return Ok(());
        };
        match kind.as_str() {
            "cancel" => self.mode = Mode::Normal,
            "select" if self.mode == Mode::Normal => {
                let idx = payload
                    .parse::<usize>()
                    .map_err(|_| "invalid entry index")?;
                if idx < self.browser.entries.len() && idx / PAGE_ROWS == self.page {
                    self.selected = Some(idx);
                    self.enter_selected()?;
                }
            }
            "parent" if self.mode == Mode::Normal => self.parent()?,
            "open" if self.mode == Mode::Normal => self.enter_selected()?,
            "page-prev" if self.mode == Mode::Normal => self.change_page(-1),
            "page-next" if self.mode == Mode::Normal => self.change_page(1),
            "app-prev" => self.change_page(-1),
            "app-next" => self.change_page(1),
            "choose" => self.choose(payload.parse::<usize>().map_err(|_| "invalid app index")?)?,
            "fallback" => self.fallback()?,
            "viewer" => self.launch_fallback(false)?,
            "terminal" => self.launch_fallback(true)?,
            "open-with" => self.show_open_with()?,
            "rename" if self.mode == Mode::Normal => self.begin_rename()?,
            "delete" if self.mode == Mode::Normal => self.begin_delete()?,
            "refresh" if self.mode == Mode::Normal => self.refresh()?,
            "rename-next" => self.confirm_name()?,
            "confirm" => self.confirm()?,
            _ => {}
        }
        Ok(())
    }
    fn parent(&mut self) -> Result<(), String> {
        if self.browser.parent()? {
            self.selected = (!self.browser.entries.is_empty()).then_some(0);
            self.page = 0;
        }
        Ok(())
    }
    fn text(&mut self, value: &str) {
        if let Mode::RenameInput { name, .. } = &mut self.mode {
            if !value.chars().any(char::is_control)
                && name.len() + value.len() <= file_manager::MAX_FILENAME_BYTES
            {
                name.push_str(value);
            }
        }
    }
    fn backspace(&mut self) {
        if let Mode::RenameInput { name, .. } = &mut self.mode {
            name.pop();
        }
    }
    fn on_key(&mut self, key: Key) -> KeyAction {
        let intent = key_intent(&key, &self.mode);
        let mut action = None;
        match intent {
            KeyIntent::Move(delta) => self.change_selection(delta),
            KeyIntent::Page(delta) => self.change_page(delta),
            KeyIntent::Text(value) => self.text(&value),
            KeyIntent::Erase => self.backspace(),
            KeyIntent::Back => {
                if self.mode != Mode::Normal {
                    self.mode = Mode::Normal;
                } else if self.browser.current == self.browser.root {
                    return KeyAction::Exit;
                } else {
                    action = Some(action_fn("parent", ""));
                }
            }
            KeyIntent::Activate => {
                action = match &self.mode {
                    Mode::Normal => Some(action_fn("open", "")),
                    Mode::OpenWith {
                        selected, handlers, ..
                    } if !handlers.is_empty() => Some(action_fn("choose", selected.to_string())),
                    Mode::RenameInput { .. } => Some(action_fn("rename-next", "")),
                    Mode::RenameConfirm { .. } | Mode::DeleteConfirm { .. } => {
                        Some(action_fn("confirm", ""))
                    }
                    Mode::Fallback { .. } => Some(action_fn("viewer", "")),
                    _ => None,
                };
            }
            KeyIntent::Rename => action = Some(action_fn("rename", "")),
            KeyIntent::Delete => action = Some(action_fn("delete", "")),
            KeyIntent::Chooser => action = Some(action_fn("open-with", "")),
            KeyIntent::Refresh => action = Some(action_fn("refresh", "")),
            KeyIntent::None => return KeyAction::NoChange,
        }
        if let Some(action) = action {
            if let Err(e) = self.activate(action) {
                self.browser.message = format!("Action refused: {e}");
            }
        }
        KeyAction::Redraw
    }
}
fn action_fn(kind: &str, payload: impl Into<String>) -> MenuAction {
    action(kind, payload)
}

// Launch only when requested by a user. Four concurrent children maximum,
// argv passed unmodified and detached reaper prevents zombies.
fn launch_argv(program: &str, args: &[&std::ffi::OsStr]) -> Result<(), String> {
    LIVE_CHILDREN
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
            (count < MAX_CHILDREN).then_some(count + 1)
        })
        .map_err(|_| "too many viewer/terminal processes already open")?;
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    match child {
        Ok(mut child) => {
            thread::spawn(move || {
                let _ = child.wait();
                LIVE_CHILDREN.fetch_sub(1, Ordering::Release);
            });
            Ok(())
        }
        Err(error) => {
            LIVE_CHILDREN.fetch_sub(1, Ordering::Release);
            Err(format!("launch {program}: {error}"))
        }
    }
}
#[derive(Debug, PartialEq, Eq)]
enum KeyAction {
    NoChange,
    Redraw,
    Exit,
}
#[derive(Debug, PartialEq, Eq)]
enum KeyIntent {
    None,
    Move(isize),
    Page(isize),
    Activate,
    Back,
    Rename,
    Delete,
    Chooser,
    Refresh,
    Text(String),
    Erase,
}
fn key_intent(key: &Key, mode: &Mode) -> KeyIntent {
    if matches!(mode, Mode::Busy { .. }) {
        return KeyIntent::None;
    }
    if let Mode::RenameInput { .. } = mode {
        return match key {
            Key::Escape => KeyIntent::Back,
            Key::Backspace => KeyIntent::Erase,
            Key::Enter => KeyIntent::Activate,
            Key::Text(t) => KeyIntent::Text(t.clone()),
            _ => KeyIntent::None,
        };
    }
    if *mode != Mode::Normal {
        return match key {
            Key::Escape | Key::Backspace => KeyIntent::Back,
            Key::Up => KeyIntent::Move(-1),
            Key::Down => KeyIntent::Move(1),
            Key::Enter | Key::Right => KeyIntent::Activate,
            Key::Text(t) if t == "[" => KeyIntent::Page(-1),
            Key::Text(t) if t == "]" => KeyIntent::Page(1),
            _ => KeyIntent::None,
        };
    }
    match key {
        Key::Up => KeyIntent::Move(-1),
        Key::Down => KeyIntent::Move(1),
        Key::Left | Key::Escape | Key::Backspace => KeyIntent::Back,
        Key::Right | Key::Enter => KeyIntent::Activate,
        Key::Raw(104) => KeyIntent::Page(-1), // Linux KEY_PAGEUP
        Key::Raw(109) => KeyIntent::Page(1),  // Linux KEY_PAGEDOWN
        Key::Text(t) => match t.as_str() {
            "j" | "J" => KeyIntent::Move(1),
            "k" | "K" => KeyIntent::Move(-1),
            "[" => KeyIntent::Page(-1),
            "]" => KeyIntent::Page(1),
            "r" | "R" => KeyIntent::Rename,
            "d" | "D" => KeyIntent::Delete,
            "o" | "O" => KeyIntent::Chooser,
            "u" | "U" => KeyIntent::Refresh,
            _ => KeyIntent::None,
        },
        _ => KeyIntent::None,
    }
}
#[allow(clippy::too_many_arguments)]
fn render(
    backend: &mut WaylandBackend,
    id: SurfaceId,
    renderer: &CairoRenderer,
    config: &ShellConfig,
    menu: &MenuModel,
    state: &MenuState,
    w: u32,
    h: u32,
    s: i32,
) -> Result<Scene, String> {
    if !(240..=2048).contains(&w)
        || !(160..=2048).contains(&h)
        || !(1..=3).contains(&s)
        || u64::from(w) * u64::from(h) * (s as u64).pow(2) * 4 > 12 * 1024 * 1024
    {
        return Err("unsafe viewport dimensions".into());
    }
    let tokens = match config.theme {
        Theme::Dark => &DARK_THEME,
        Theme::Light => &LIGHT_THEME,
    };
    let (scene, mut buffer) = render_menu_follow_selection(
        renderer,
        menu,
        state,
        Viewport::new(f64::from(w), f64::from(h), f64::from(s)),
        tokens,
        RenderOptions::default(),
        None,
    )?;
    let info = buffer.info();
    let result = buffer
        .with_argb32_bytes(|pixels, _| {
            backend.present(
                id,
                Frame {
                    width: info.width as u32,
                    height: info.height as u32,
                    stride: info.stride as usize,
                    format: PixelFormat::Argb8888,
                    pixels,
                },
            )
        })
        .map_err(|e| e.to_string())?;
    match result {
        Ok(()) | Err(BackendError::WouldBlock) => Ok(scene),
        Err(e) => Err(e.to_string()),
    }
}
fn hit(scene: &Option<Scene>, menu: &MenuModel, x: f64, y: f64) -> Option<MenuAction> {
    menu.item_by_id(scene.as_ref()?.hit_test(x, y)?)?
        .action
        .clone()
}
fn release_inside(
    press: &mut Option<MenuAction>,
    release: Option<MenuAction>,
) -> Option<MenuAction> {
    if press.as_ref() == release.as_ref() {
        press.take()
    } else {
        *press = None;
        None
    }
}
// A short poll only while a file job runs. Idle remains fully event-driven.
// Wayland backend owns all queue dispatch; this merely multiplexes its fd.
#[cfg(unix)]
fn dispatch_while_opening(backend: &mut WaylandBackend) -> Result<(), String> {
    use std::os::raw::{c_int, c_short};
    #[repr(C)]
    struct PollFd {
        fd: c_int,
        events: c_short,
        revents: c_short,
    }
    unsafe extern "C" {
        fn poll(fds: *mut PollFd, nfds: usize, timeout: c_int) -> c_int;
    }
    const POLLIN: c_short = 1;
    backend.flush().map_err(|e| e.to_string())?;
    let mut fd = PollFd {
        fd: backend.poll_fd(),
        events: POLLIN,
        revents: 0,
    };
    let rc = unsafe { poll(&mut fd, 1, 40) };
    if rc < 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::Interrupted {
            return Err(error.to_string());
        }
    } else if rc > 0 && fd.revents != 0 {
        backend.blocking_dispatch().map_err(|e| e.to_string())?;
    } else {
        backend.dispatch_pending().map_err(|e| e.to_string())?;
    }
    Ok(())
}
fn touch_release(press: &mut Option<(i32, MenuAction)>, id: i32) -> Option<MenuAction> {
    if press.as_ref().is_some_and(|(tid, _)| *tid == id) {
        press.take().map(|(_, action)| action)
    } else {
        None
    }
}
fn swipe_page(start: f64, end: f64) -> Option<isize> {
    if !start.is_finite() || !end.is_finite() {
        return None;
    }
    if start - end >= 72.0 {
        Some(1)
    } else if end - start >= 72.0 {
        Some(-1)
    } else {
        None
    }
}
fn run(opts: Options) -> Result<(), String> {
    let config = load_config_or_default(opts.config.as_deref())?;
    let mut ui = Ui::new(&opts)?;
    if !opts.live {
        println!("{}{}", HELP, ui.browser.current.display());
        return Ok(());
    }
    let mut backend = WaylandBackend::connect().map_err(|e| e.to_string())?;
    if !backend.capabilities().layer_shell {
        return Err("layer-shell unavailable".into());
    }
    let output = backend.outputs().into_iter().next();
    let id = backend
        .create_menu(MenuConfig {
            width: config.menu_width.clamp(320, 720),
            height: 640,
            margin_top: config.panel_height as i32,
            margin_left: 0,
            output: output.as_ref().map(|o| o.id),
            namespace: "nuraloumi-files".into(),
        })
        .map_err(|e| e.to_string())?;
    let renderer = CairoRenderer::default();
    let mut menu = ui.menu();
    let mut geometry = None;
    let mut scene = None;
    let mut touch: Option<(i32, MenuAction)> = None;
    let mut touch_swipe: Option<(i32, f64, Option<isize>)> = None;
    let mut pointer: Option<MenuAction> = None;
    loop {
        if ui.busy() {
            dispatch_while_opening(&mut backend)?;
        } else {
            backend.blocking_dispatch().map_err(|e| e.to_string())?;
        }
        // Worker replies must repaint even when the compositor sends no input.
        if ui.poll_worker() {
            menu = ui.menu();
            let state = ui.state(&menu);
            if let Some((w, h, s)) = geometry {
                scene = Some(render(
                    &mut backend,
                    id,
                    &renderer,
                    &config,
                    &menu,
                    &state,
                    w,
                    h,
                    s,
                )?);
            }
        }
        for incoming in backend.drain_events().collect::<Vec<_>>() {
            if incoming.surface.is_some() && incoming.surface != Some(id) {
                continue;
            }
            let mut redraw = false;
            let mut requested = None;
            match incoming.event {
                PlatformEvent::Configure {
                    width,
                    height,
                    scale,
                } => {
                    geometry = Some((width, height, scale));
                    redraw = true;
                }
                PlatformEvent::Close => {
                    let _ = backend.destroy_surface(id);
                    return Ok(());
                }
                PlatformEvent::Key { pressed: true, key } => match ui.on_key(key) {
                    KeyAction::Exit => {
                        let _ = backend.destroy_surface(id);
                        return Ok(());
                    }
                    KeyAction::Redraw => redraw = true,
                    KeyAction::NoChange => {}
                },
                PlatformEvent::TouchDown { id: tid, x, y } => {
                    if touch_swipe.is_none() {
                        // A one-finger vertical swipe pages; release-inside
                        // remains the only way to activate an item.
                        if matches!(ui.mode, Mode::Normal | Mode::OpenWith { .. }) {
                            touch_swipe = Some((tid, y, None));
                            touch = hit(&scene, &menu, x, y).map(|a| (tid, a));
                        }
                    }
                }
                PlatformEvent::TouchMotion { id: tid, x, y } => {
                    if let Some((id, origin, direction)) = &mut touch_swipe {
                        if *id == tid {
                            let candidate = swipe_page(*origin, y);
                            if candidate.is_some() {
                                *direction = candidate;
                                touch = None;
                            }
                        }
                    }
                    if touch.as_ref().is_some_and(|(id, a)| {
                        *id == tid && hit(&scene, &menu, x, y).as_ref() != Some(a)
                    }) {
                        touch = None;
                    }
                }
                PlatformEvent::TouchUp { id: tid } => {
                    if touch_swipe.as_ref().is_some_and(|(id, _, _)| *id == tid) {
                        let direction = touch_swipe.take().and_then(|(_, _, direction)| direction);
                        if let Some(direction) = direction {
                            ui.change_page(direction);
                            touch = None;
                            redraw = true;
                        } else {
                            requested = touch_release(&mut touch, tid);
                        }
                    }
                }
                PlatformEvent::TouchCancel { .. } => {
                    touch = None;
                    touch_swipe = None;
                }
                PlatformEvent::PointerButton {
                    pressed: true,
                    x,
                    y,
                    button: 0x110,
                } => pointer = hit(&scene, &menu, x, y),
                PlatformEvent::PointerButton {
                    pressed: false,
                    x,
                    y,
                    button: 0x110,
                } => requested = release_inside(&mut pointer, hit(&scene, &menu, x, y)),
                _ => {}
            }
            if let Some(action) = requested {
                if let Err(error) = ui.activate(action) {
                    ui.browser.message = format!("Action refused: {error}");
                }
                redraw = true;
            }
            if redraw {
                // Any model change invalidates a held pointer/touch press.
                touch = None;
                touch_swipe = None;
                pointer = None;
                menu = ui.menu();
                let state = ui.state(&menu);
                if let Some((w, h, s)) = geometry {
                    scene = Some(render(
                        &mut backend,
                        id,
                        &renderer,
                        &config,
                        &menu,
                        &state,
                        w,
                        h,
                        s,
                    )?);
                }
            }
        }
    }
}
fn main() -> ExitCode {
    match parse(env::args().skip(1)).and_then(|v| v.map(run).transpose()) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nuraloumi-files: {e}");
            ExitCode::FAILURE
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};
    fn temp() -> PathBuf {
        let id = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = env::temp_dir().join(format!("files-live-{}-{id}", std::process::id()));
        fs::create_dir(&path).unwrap();
        path
    }
    fn opts(root: PathBuf) -> Options {
        Options {
            live: false,
            config: None,
            root,
            capabilities: Capabilities::default(),
            open_with: None,
        }
    }
    struct Fake {
        decision: OpenOutcome,
        handlers: Vec<DesktopHandler>,
    }
    impl FileOps for Fake {
        fn open_default(&mut self, _: &Path) -> OpenOutcome {
            self.decision.clone()
        }
        fn handlers(&mut self, _: &Path) -> Vec<DesktopHandler> {
            self.handlers.clone()
        }
        fn open_with(&mut self, _: &Path, id: &str) -> Result<String, String> {
            Ok(format!("app:{id}"))
        }
        fn describe(&mut self, _: &Path) -> Result<String, String> {
            Ok("text/plain, ASCII".into())
        }
    }
    fn fake(decision: OpenOutcome, handlers: Vec<DesktopHandler>) -> Box<dyn FileOps> {
        Box::new(Fake { decision, handlers })
    }
    fn handler(id: &str) -> DesktopHandler {
        DesktopHandler {
            id: id.to_owned(),
            name: id.to_owned(),
            desktop_file: PathBuf::from(id),
        }
    }
    #[test]
    fn capabilities_default_safe() {
        assert!(
            !parse(vec!["--live".into()])
                .unwrap()
                .unwrap()
                .capabilities
                .delete
        );
        assert!(parse(vec!["--open-with".into(), "../../malicious.desktop".into()]).is_err());
    }
    #[test]
    fn paging_selection_and_keyboard_mapping() {
        let root = temp();
        for idx in 0..32 {
            fs::write(root.join(format!("{idx:02}.txt")), "x").unwrap();
        }
        let mut ui = Ui::with_ops(
            &opts(root.clone()),
            fake(
                OpenOutcome::NeedsFallback {
                    reason: "none".into(),
                },
                vec![],
            ),
        )
        .unwrap();
        assert_eq!(Ui::page_count(ui.browser.entries.len()), 8);
        ui.change_selection(9);
        assert_eq!(ui.selected, Some(9));
        assert_eq!(ui.page, 2);
        assert!(ui.menu().item_by_id("entry-9").is_some());
        assert!(ui.menu().item_by_id("entry-0").is_none());
        ui.change_page(-2);
        assert_eq!(ui.selected, Some(0));
        assert_eq!(ui.page, 0);
        assert_eq!(
            key_intent(&Key::Raw(109), &Mode::Normal),
            KeyIntent::Page(1)
        );
        assert_eq!(key_intent(&Key::Left, &Mode::Normal), KeyIntent::Back);
        assert_eq!(ui.on_key(Key::Text("]".into())), KeyAction::Redraw);
        assert_eq!(ui.page, 1);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn default_to_interactive_chooser_and_fallback() {
        let root = temp();
        fs::write(root.join("hello.txt"), "hi").unwrap();
        let apps = vec![handler("view.desktop")];
        let mut ui = Ui::with_ops(
            &opts(root.clone()),
            fake(
                OpenOutcome::NeedsOpenWith {
                    reason: "no XDG default".into(),
                    handlers: apps.clone(),
                },
                apps,
            ),
        )
        .unwrap();
        ui.enter_selected().unwrap();
        assert!(matches!(ui.mode, Mode::OpenWith { .. }));
        assert!(ui.menu().item_by_id("app-0").is_some());
        ui.choose(0).unwrap();
        assert!(matches!(ui.mode, Mode::Normal));
        assert!(ui.browser.message.contains("app:view.desktop"));
        ui.show_open_with().unwrap();
        ui.fallback().unwrap();
        assert!(matches!(ui.mode, Mode::Fallback { .. }));
        assert!(ui.menu().item_by_id("viewer").is_some());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn confirmation_invalidates_on_identity_change() {
        let root = temp();
        fs::write(root.join("a.txt"), "a").unwrap();
        let mut options = opts(root.clone());
        options.capabilities.delete = true;
        let mut ui = Ui::with_ops(
            &options,
            fake(OpenOutcome::NeedsFallback { reason: "x".into() }, vec![]),
        )
        .unwrap();
        ui.begin_delete().unwrap();
        fs::remove_file(root.join("a.txt")).unwrap();
        fs::write(root.join("a.txt"), "replaced").unwrap();
        assert!(ui.confirm().is_err());
        assert_eq!(fs::read(root.join("a.txt")).unwrap(), b"replaced");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn touch_swipes_page_without_clicking() {
        assert_eq!(swipe_page(160.0, 80.0), Some(1));
        assert_eq!(swipe_page(80.0, 160.0), Some(-1));
        assert_eq!(swipe_page(160.0, 90.0), None);
        assert_eq!(swipe_page(f64::NAN, 5.0), None);
        assert_eq!(swipe_page(5.0, f64::INFINITY), None);
    }
    #[test]
    fn pointer_release_requires_same_region() {
        let a = action("select", "0");
        let b = action("select", "1");
        let mut held = Some(a.clone());
        assert_eq!(release_inside(&mut held, Some(b)), None);
        assert!(held.is_none());
        held = Some(a.clone());
        assert_eq!(release_inside(&mut held, Some(a.clone())), Some(a));
        assert!(held.is_none());

        let mut pressed = Some((13, action("open", "")));
        assert!(touch_release(&mut pressed, 14).is_none());
        assert!(pressed.is_some());
        assert_eq!(touch_release(&mut pressed, 13), Some(action("open", "")));
        assert!(pressed.is_none());
    }

    #[test]
    fn async_default_fallback_preserves_responsive_ui_and_chooser() {
        use std::time::Duration;
        let root = temp();
        fs::write(root.join("text.txt"), "hello").unwrap();
        let mut ui = Ui::with_ops(
            &opts(root.clone()),
            fake(
                OpenOutcome::NeedsFallback {
                    reason: "no default".into(),
                },
                vec![handler("editor.desktop")],
            ),
        )
        .unwrap();
        ui.worker = Some(OpenWorker::new(ui.ops.take().unwrap()));
        ui.enter_selected().unwrap();
        assert!(ui.busy());
        assert!(ui.menu().item_by_id("opening").is_some());
        assert_eq!(key_intent(&Key::Escape, &ui.mode), KeyIntent::None);
        let reply = ui
            .worker
            .as_ref()
            .unwrap()
            .replies
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        ui.apply_reply(reply).unwrap();
        // The fallback handler discovery is a second bounded job.
        assert!(ui.busy());
        let reply = ui
            .worker
            .as_ref()
            .unwrap()
            .replies
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        ui.apply_reply(reply).unwrap();
        assert!(matches!(ui.mode, Mode::OpenWith { .. }));
        assert!(ui.menu().item_by_id("app-0").is_some());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn queued_file_replacement_is_rejected_before_provider_execution() {
        let root = temp();
        let path = root.join("record.txt");
        fs::write(&path, "original").unwrap();
        let browser = Browser::new(&root, Capabilities::default()).unwrap();
        let expected = browser.entries[0].clone();
        fs::remove_file(&path).unwrap();
        fs::write(&path, "changed").unwrap();
        let reply = perform_job(
            fake(
                OpenOutcome::Opened {
                    handler: "fake".into(),
                },
                vec![],
            )
            .as_mut(),
            Job {
                index: 0,
                expected,
                path,
                kind: JobKind::Default,
            },
        );
        assert!(matches!(reply.kind, ReplyKind::Invalid(_)));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn paging_controls_are_real_clipped_touch_targets() {
        let root = temp();
        for i in 0..12 {
            fs::write(root.join(format!("file-{i:02}")), "ok").unwrap();
        }
        let ui = Ui::with_ops(
            &opts(root.clone()),
            fake(
                OpenOutcome::NeedsFallback {
                    reason: "none".into(),
                },
                vec![],
            ),
        )
        .unwrap();
        let menu = ui.menu();
        let state = ui.state(&menu);
        let viewport = Viewport::new(480.0, 640.0, 1.0);
        let (scene, _) = render_menu_follow_selection(
            &CairoRenderer::default(),
            &menu,
            &state,
            viewport,
            &DARK_THEME,
            RenderOptions::default(),
            None,
        )
        .unwrap();
        let next = scene.hits.iter().find(|hit| hit.item_id == "next").unwrap();
        assert!(next.rect.height >= 48.0);
        assert!(next.rect.y >= 0.0 && next.rect.bottom() <= viewport.height);
        assert_eq!(
            scene.hit_test(next.rect.x + 5.0, next.rect.y + 5.0),
            Some("next")
        );
        for action in ["parent", "next", "open-with"] {
            let hit = scene.hits.iter().find(|hit| hit.item_id == action).unwrap();
            assert!(hit.rect.height >= 48.0, "{action} has a small target");
            assert!(
                hit.rect.y >= 0.0 && hit.rect.bottom() <= viewport.height,
                "{action} is offscreen: {:?}",
                hit.rect
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
}
