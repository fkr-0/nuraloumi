//! Freedesktop notification daemon. Independent of the Wayland UI and dunst.
//! Start only once per user session. Never replace or queue behind another owner.
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::interface;
use zbus::zvariant::OwnedValue;

const BUS: &str = "org.freedesktop.Notifications";
const OBJECT: &str = "/org/freedesktop/Notifications";
const LIMIT: usize = 64;
const MAX_TEXT: usize = 4096;
const MAX_ACTIONS: usize = 32;

#[derive(Clone, serde::Serialize)]
struct Item {
    id: u32,
    app: String,
    summary: String,
    body: String,
    actions: Vec<String>,
    default_action: Option<String>,
    active: bool,
    #[serde(skip)]
    expires: Option<Instant>,
}

#[derive(Default)]
struct State {
    next_id: u32,
    items: VecDeque<Item>,
}

impl State {
    fn new_id(&mut self) -> u32 {
        loop {
            self.next_id = self.next_id.wrapping_add(1).max(1);
            if !self.items.iter().any(|i| i.id == self.next_id) {
                return self.next_id;
            }
        }
    }

    fn close(&mut self, id: u32) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.id == id && i.active) {
            item.active = false;
            item.expires = None;
            return true;
        }
        false
    }

    fn insert(&mut self, mut item: Item, replace_id: u32) -> (u32, Vec<u32>) {
        let mut evicted = Vec::new();
        let id = if replace_id != 0 && self.items.iter().any(|i| i.id == replace_id && i.active) {
            self.items.retain(|i| i.id != replace_id);
            replace_id
        } else {
            self.new_id()
        };
        item.id = id;
        self.items.push_front(item);
        while self.items.len() > LIMIT {
            if let Some(old) = self.items.pop_back() {
                evicted.push(old.id);
            }
        }
        (id, evicted)
    }

    fn remove(&mut self, id: u32) -> bool {
        let before = self.items.len();
        self.items.retain(|i| i.id != id);
        before != self.items.len()
    }

    fn expired(&mut self, now: Instant) -> Vec<u32> {
        let ids = self
            .items
            .iter()
            .filter(|i| i.active && i.expires.is_some_and(|e| e <= now))
            .map(|i| i.id)
            .collect::<Vec<_>>();
        for id in &ids {
            self.close(*id);
        }
        ids
    }
}

struct Notifications(Arc<Mutex<State>>);

#[interface(name = "org.freedesktop.Notifications")]
impl Notifications {
    fn get_capabilities(&self) -> Vec<&'static str> {
        vec!["actions", "body"]
    }

    fn get_server_information(&self) -> (&'static str, &'static str, &'static str, &'static str) {
        ("NuraLoumi", "NuraLoumi", env!("CARGO_PKG_VERSION"), "1.2")
    }

    #[allow(clippy::too_many_arguments)]
    async fn notify(
        &self,
        app_name: &str,
        replaces_id: u32,
        _app_icon: &str,
        summary: &str,
        body: &str,
        actions: Vec<String>,
        _hints: HashMap<String, OwnedValue>,
        expire_timeout: i32,
        #[zbus(signal_context)] ctxt: zbus::object_server::SignalContext<'_>,
    ) -> zbus::fdo::Result<u32> {
        if app_name.len() > MAX_TEXT
            || summary.len() > MAX_TEXT
            || body.len() > MAX_TEXT
            || actions.len() > MAX_ACTIONS
            || actions.iter().any(|a| a.len() > MAX_TEXT)
            || actions.len() & 1 != 0
        {
            return Err(zbus::fdo::Error::InvalidArgs(
                "notification exceeds bounded limits".into(),
            ));
        }
        let pairs = actions
            .chunks_exact(2)
            .map(|pair| pair[0].clone())
            .collect::<Vec<_>>();
        // Freedesktop default timeout -1: choose a bounded five-second lifetime.
        // Zero means resident; positive values are clamped to avoid overflow.
        let timeout = match expire_timeout {
            0 => None,
            n if n < 0 => Some(Duration::from_millis(5_000)),
            n => Some(Duration::from_millis((n as u64).clamp(100, 120_000))),
        };
        let item = Item {
            id: 0,
            app: app_name.into(),
            summary: summary.into(),
            body: body.into(),
            default_action: pairs.iter().find(|a| a.as_str() == "default").cloned(),
            actions: pairs,
            active: true,
            expires: timeout.map(|d| Instant::now() + d),
        };
        let (id, evicted) = self
            .0
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("lock poisoned".into()))?
            .insert(item, replaces_id);
        for id in evicted {
            Self::notification_closed(&ctxt, id, 4)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        Self::notification_added(&ctxt, id)
            .await
            .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        Ok(id)
    }

    async fn close_notification(
        &self,
        id: u32,
        #[zbus(signal_context)] ctxt: zbus::object_server::SignalContext<'_>,
    ) -> zbus::fdo::Result<()> {
        let removed = self
            .0
            .lock()
            .map_err(|_| zbus::fdo::Error::Failed("lock poisoned".into()))?
            .close(id);
        if removed {
            Self::notification_closed(&ctxt, id, 3)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        Ok(())
    }

    /// NuraLoumi-specific read-only history for the center, not an FDO method.
    fn get_history(&self) -> String {
        self.0
            .lock()
            .map(|s| serde_json::to_string(&s.items).unwrap_or_default())
            .unwrap_or_else(|_| "[]".into())
    }

    async fn dismiss_history(
        &self,
        id: u32,
        #[zbus(signal_context)] ctxt: zbus::object_server::SignalContext<'_>,
    ) -> zbus::fdo::Result<bool> {
        let (removed, active) = {
            let mut state = self
                .0
                .lock()
                .map_err(|_| zbus::fdo::Error::Failed("lock poisoned".into()))?;
            let active = state.items.iter().any(|i| i.id == id && i.active);
            (state.remove(id), active)
        };
        if removed && active {
            Self::notification_closed(&ctxt, id, 2)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        Ok(removed)
    }

    async fn invoke_action(
        &self,
        id: u32,
        action: &str,
        #[zbus(signal_context)] ctxt: zbus::object_server::SignalContext<'_>,
    ) -> zbus::fdo::Result<bool> {
        let exists = {
            let mut state = self
                .0
                .lock()
                .map_err(|_| zbus::fdo::Error::Failed("lock poisoned".into()))?;
            let found = state
                .items
                .iter()
                .any(|i| i.id == id && i.active && i.actions.iter().any(|a| a == action));
            if found {
                state.remove(id);
            }
            found
        };
        if exists {
            Self::action_invoked(&ctxt, id, action)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
            Self::notification_closed(&ctxt, id, 2)
                .await
                .map_err(|e| zbus::fdo::Error::Failed(e.to_string()))?;
        }
        Ok(exists)
    }

    #[zbus(signal)]
    async fn notification_added(
        ctxt: &zbus::object_server::SignalContext<'_>,
        id: u32,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn action_invoked(
        ctxt: &zbus::object_server::SignalContext<'_>,
        id: u32,
        action: &str,
    ) -> zbus::Result<()>;
    #[zbus(signal)]
    async fn notification_closed(
        ctxt: &zbus::object_server::SignalContext<'_>,
        id: u32,
        reason: u32,
    ) -> zbus::Result<()>;
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let state = Arc::new(Mutex::new(State::default()));
    let conn = zbus::blocking::connection::Builder::session()?
        .serve_at(OBJECT, Notifications(state.clone()))?
        .build()?;
    let flags = RequestNameFlags::DoNotQueue.into();
    let reply = conn.request_name_with_flags(BUS, flags)?;
    if reply != RequestNameReply::PrimaryOwner && reply != RequestNameReply::AlreadyOwner {
        return Err(format!("{BUS} already owned; refusing to queue or replace").into());
    }
    eprintln!("nuraloumi-notificationd: owns {BUS}");
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let expired = state
            .lock()
            .map_err(|_| "notification state poisoned")?
            .expired(Instant::now());
        for id in expired {
            conn.emit_signal(None::<&str>, OBJECT, BUS, "NotificationClosed", &(id, 1u32))?;
        }
    }
}

fn main() {
    if let Err(error) = run() {
        eprintln!("nuraloumi-notificationd: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn item() -> Item {
        Item {
            id: 0,
            app: "test".into(),
            summary: "summary".into(),
            body: "".into(),
            actions: vec![],
            default_action: None,
            active: true,
            expires: None,
        }
    }
    #[test]
    fn replacement_and_storage_bound() {
        let mut s = State::default();
        let (id, _) = s.insert(item(), 0);
        assert_eq!(s.insert(item(), id).0, id);
        assert_eq!(s.items.len(), 1);
        for _ in 0..LIMIT {
            s.insert(item(), 0);
        }
        assert_eq!(s.items.len(), LIMIT);
        assert!(!s.items.iter().any(|i| i.id == id));
        assert!(!s.remove(id));
    }
    #[test]
    fn expiry_is_one_shot() {
        let mut s = State::default();
        let mut i = item();
        i.expires = Some(Instant::now() - Duration::from_secs(1));
        let (id, _) = s.insert(i, 0);
        assert_eq!(s.expired(Instant::now()), vec![id]);
        assert!(s.expired(Instant::now()).is_empty());
        assert_eq!(s.items.len(), 1);
        assert!(!s.items[0].active);
    }
}
