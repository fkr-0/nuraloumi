use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    fmt,
    str::FromStr,
};

use wayland_client::{backend::ObjectId, Proxy};
use wayland_protocols::ext::workspace::v1::client::{
    ext_workspace_handle_v1::ExtWorkspaceHandleV1, ext_workspace_manager_v1::ExtWorkspaceManagerV1,
};

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WorkspaceId(u64);

impl WorkspaceId {
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for WorkspaceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ws:{:016x}", self.0)
    }
}

impl FromStr for WorkspaceId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let raw = value
            .strip_prefix("ws:")
            .ok_or("workspace id must start with ws:")?;
        if raw.len() != 16 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("workspace id must contain exactly 16 hexadecimal digits");
        }
        u64::from_str_radix(raw, 16)
            .map(Self)
            .map_err(|_| "invalid workspace id")
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceState {
    pub active: bool,
    pub urgent: bool,
    pub hidden: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceInfo {
    pub id: WorkspaceId,
    pub name: String,
    pub protocol_id: Option<String>,
    pub state: WorkspaceState,
    pub can_activate: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceEvent {
    Added(WorkspaceInfo),
    Changed(WorkspaceInfo),
    Removed(WorkspaceId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceCapabilities {
    pub list: bool,
    pub activate: bool,
}

#[derive(Clone, Debug, Default)]
struct PendingWorkspace {
    name: String,
    protocol_id: Option<String>,
    state: WorkspaceState,
    can_activate: bool,
    removed: bool,
}

impl PendingWorkspace {
    fn from_info(info: &WorkspaceInfo) -> Self {
        Self {
            name: info.name.clone(),
            protocol_id: info.protocol_id.clone(),
            state: info.state,
            can_activate: info.can_activate,
            removed: false,
        }
    }

    fn into_info(self, id: WorkspaceId) -> WorkspaceInfo {
        let name = if self.name.trim().is_empty() {
            self.protocol_id
                .clone()
                .unwrap_or_else(|| format!("Workspace {}", id.raw()))
        } else {
            self.name
        };
        WorkspaceInfo {
            id,
            name,
            protocol_id: self.protocol_id,
            state: self.state,
            can_activate: self.can_activate,
        }
    }
}

struct WorkspaceRecord {
    handle: ExtWorkspaceHandleV1,
    pending: PendingWorkspace,
    committed: Option<WorkspaceInfo>,
}

impl WorkspaceRecord {
    fn new(handle: ExtWorkspaceHandleV1) -> Self {
        Self {
            handle,
            pending: PendingWorkspace::default(),
            committed: None,
        }
    }
}

fn workspace_is_activatable(committed: Option<&WorkspaceInfo>, pending: &PendingWorkspace) -> bool {
    !pending.removed
        && pending.can_activate
        && committed.is_some_and(|workspace| workspace.can_activate)
}

#[derive(Default)]
pub(crate) struct WorkspaceStore {
    next_id: u64,
    global: Option<(u32, u32)>,
    manager: Option<ExtWorkspaceManagerV1>,
    records: BTreeMap<WorkspaceId, WorkspaceRecord>,
    objects: HashMap<ObjectId, WorkspaceId>,
    events: VecDeque<WorkspaceEvent>,
}

impl WorkspaceStore {
    pub(crate) fn record_global(&mut self, name: u32, version: u32) {
        self.global = Some((name, version));
    }

    pub(crate) fn global(&self) -> Option<(u32, u32)> {
        self.global
    }

    pub(crate) fn has_binding(&self) -> bool {
        self.manager.is_some()
    }

    pub(crate) fn bind(&mut self, manager: ExtWorkspaceManagerV1) {
        self.manager = Some(manager);
    }

    pub(crate) fn remove_global(&mut self, name: u32) {
        if self.global.is_some_and(|(global, _)| global == name) {
            self.global = None;
            self.finish();
        }
    }

    pub(crate) fn finish(&mut self) {
        self.manager = None;
        self.global = None;
        let ids: Vec<_> = self.records.keys().copied().collect();
        for id in ids {
            if let Some(record) = self.records.remove(&id) {
                self.objects.retain(|_, value| *value != id);
                if record.committed.is_some() {
                    self.events.push_back(WorkspaceEvent::Removed(id));
                }
            }
        }
    }

    pub(crate) fn capabilities(&self) -> WorkspaceCapabilities {
        WorkspaceCapabilities {
            list: self.manager.is_some(),
            activate: self
                .records
                .values()
                .any(|record| workspace_is_activatable(record.committed.as_ref(), &record.pending)),
        }
    }

    pub(crate) fn workspaces(&self) -> Vec<WorkspaceInfo> {
        self.records
            .values()
            .filter_map(|record| record.committed.clone())
            .collect()
    }

    pub(crate) fn drain_events(&mut self) -> impl Iterator<Item = WorkspaceEvent> + '_ {
        self.events.drain(..)
    }

    pub(crate) fn manager(&self) -> Option<ExtWorkspaceManagerV1> {
        self.manager.clone()
    }

    pub(crate) fn activatable_handle(&self, id: WorkspaceId) -> Option<ExtWorkspaceHandleV1> {
        let record = self.records.get(&id)?;
        if !workspace_is_activatable(record.committed.as_ref(), &record.pending) {
            return None;
        }
        Some(record.handle.clone())
    }

    pub(crate) fn register(&mut self, handle: ExtWorkspaceHandleV1) {
        self.next_id = self.next_id.saturating_add(1).max(1);
        let id = WorkspaceId(self.next_id);
        self.objects.insert(handle.id(), id);
        self.records.insert(id, WorkspaceRecord::new(handle));
    }

    pub(crate) fn set_protocol_id(&mut self, object: &ObjectId, protocol_id: String) {
        if let Some(record) = self.record_mut(object) {
            record.pending.protocol_id = Some(protocol_id);
        }
    }

    pub(crate) fn set_name(&mut self, object: &ObjectId, name: String) {
        if let Some(record) = self.record_mut(object) {
            record.pending.name = name;
        }
    }

    pub(crate) fn set_state(&mut self, object: &ObjectId, raw: u32) {
        if let Some(record) = self.record_mut(object) {
            record.pending.state = workspace_state(raw);
        }
    }

    pub(crate) fn set_capabilities(&mut self, object: &ObjectId, raw: u32) {
        if let Some(record) = self.record_mut(object) {
            record.pending.can_activate = workspace_can_activate(raw);
        }
    }

    pub(crate) fn mark_removed(&mut self, object: &ObjectId) {
        if let Some(record) = self.record_mut(object) {
            record.pending.removed = true;
        }
    }

    pub(crate) fn commit_done(&mut self) {
        let ids: Vec<_> = self.records.keys().copied().collect();
        for id in ids {
            let removed = self
                .records
                .get(&id)
                .is_some_and(|record| record.pending.removed);
            if removed {
                if let Some(record) = self.records.remove(&id) {
                    self.objects.retain(|_, value| *value != id);
                    if record.committed.is_some() {
                        self.events.push_back(WorkspaceEvent::Removed(id));
                    }
                    if record.handle.is_alive() {
                        record.handle.destroy();
                    }
                }
                continue;
            }

            let Some(record) = self.records.get_mut(&id) else {
                continue;
            };
            if let Some(event) = publish_workspace(id, &mut record.committed, &mut record.pending) {
                self.events.push_back(event);
            }
        }
    }

    fn record_mut(&mut self, object: &ObjectId) -> Option<&mut WorkspaceRecord> {
        let id = self.objects.get(object)?;
        self.records.get_mut(id)
    }
}

fn workspace_state(raw: u32) -> WorkspaceState {
    WorkspaceState {
        active: raw & 1 != 0,
        urgent: raw & 2 != 0,
        hidden: raw & 4 != 0,
    }
}

fn workspace_can_activate(raw: u32) -> bool {
    raw & 1 != 0
}

fn publish_workspace(
    id: WorkspaceId,
    committed: &mut Option<WorkspaceInfo>,
    pending: &mut PendingWorkspace,
) -> Option<WorkspaceEvent> {
    let info = pending.clone().into_info(id);
    let event = match committed.as_ref() {
        None => Some(WorkspaceEvent::Added(info.clone())),
        Some(previous) if previous != &info => Some(WorkspaceEvent::Changed(info.clone())),
        Some(_) => None,
    };
    *committed = Some(info.clone());
    *pending = PendingWorkspace::from_info(&info);
    event
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_ids_round_trip_without_names() {
        let id = WorkspaceId(0x1234);
        assert_eq!(id.to_string(), "ws:0000000000001234");
        assert_eq!(id.to_string().parse::<WorkspaceId>(), Ok(id));
        assert!("Desktop 1".parse::<WorkspaceId>().is_err());
        assert!("1".parse::<WorkspaceId>().is_err());
    }

    #[test]
    fn state_bits_are_decoded_without_unknown_bit_failure() {
        let state = workspace_state(0b1001);
        assert!(state.active);
        assert!(!state.urgent);
        assert!(!state.hidden);
    }

    #[test]
    fn activation_never_leads_the_done_boundary() {
        let id = WorkspaceId(1);
        let committed = WorkspaceInfo {
            id,
            name: "One".into(),
            protocol_id: Some("one".into()),
            state: WorkspaceState::default(),
            can_activate: false,
        };
        let mut pending = PendingWorkspace::from_info(&committed);

        pending.can_activate = true;
        assert!(!workspace_is_activatable(Some(&committed), &pending));

        let committed = WorkspaceInfo {
            can_activate: true,
            ..committed
        };
        pending.can_activate = true;
        assert!(workspace_is_activatable(Some(&committed), &pending));

        pending.can_activate = false;
        assert!(!workspace_is_activatable(Some(&committed), &pending));

        pending.can_activate = true;
        pending.removed = true;
        assert!(!workspace_is_activatable(Some(&committed), &pending));
    }

    #[test]
    fn publication_is_atomic_at_done() {
        let id = WorkspaceId(2);
        let original = WorkspaceInfo {
            id,
            name: "One".into(),
            protocol_id: Some("stable-one".into()),
            state: WorkspaceState::default(),
            can_activate: true,
        };
        let mut committed = Some(original.clone());
        let mut pending = PendingWorkspace::from_info(&original);
        pending.name = "Two".into();
        pending.state.active = true;

        assert_eq!(committed.as_ref(), Some(&original));

        let event = publish_workspace(id, &mut committed, &mut pending);
        assert!(matches!(
            event,
            Some(WorkspaceEvent::Changed(WorkspaceInfo {
                ref name,
                state: WorkspaceState { active: true, .. },
                ..
            })) if name == "Two"
        ));
        assert_eq!(
            committed.as_ref().map(|info| info.name.as_str()),
            Some("Two")
        );
        assert!(committed.as_ref().is_some_and(|info| info.state.active));
        assert!(publish_workspace(id, &mut committed, &mut pending).is_none());
    }

    #[test]
    fn capability_bits_ignore_unknown_flags() {
        assert!(workspace_can_activate(0b1001));
        assert!(!workspace_can_activate(0b1110));
    }
}
