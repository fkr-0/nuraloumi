use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt,
    str::FromStr,
};

use wayland_client::{backend::ObjectId, Proxy};
use wayland_protocols::ext::foreign_toplevel_list::v1::client::{
    ext_foreign_toplevel_handle_v1::ExtForeignToplevelHandleV1,
    ext_foreign_toplevel_list_v1::ExtForeignToplevelListV1,
};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1::ZwlrForeignToplevelHandleV1,
    zwlr_foreign_toplevel_manager_v1::ZwlrForeignToplevelManagerV1,
};

use crate::OutputId;

#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ToplevelId(u64);

impl ToplevelId {
    pub const fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for ToplevelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "tl:{:016x}", self.0)
    }
}

impl FromStr for ToplevelId {
    type Err = &'static str;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let raw = value
            .strip_prefix("tl:")
            .ok_or("toplevel id must start with tl:")?;
        if raw.len() != 16 || !raw.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("toplevel id must contain exactly 16 hexadecimal digits");
        }
        u64::from_str_radix(raw, 16)
            .map(Self)
            .map_err(|_| "invalid toplevel id")
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToplevelState {
    pub activated: bool,
    pub fullscreen: bool,
    pub maximized: bool,
    pub minimized: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ToplevelSource {
    WlrManagement,
    ExtList,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToplevelInfo {
    pub id: ToplevelId,
    pub title: String,
    pub app_id: Option<String>,
    pub state: ToplevelState,
    pub outputs: Vec<OutputId>,
    pub source: ToplevelSource,
    pub protocol_identifier: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToplevelEvent {
    Added(ToplevelInfo),
    Changed(ToplevelInfo),
    Removed(ToplevelId),
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ToplevelCapabilities {
    pub list: bool,
    pub state: bool,
    pub activate: bool,
    pub fullscreen: bool,
    pub close: bool,
}

#[derive(Clone, Debug, Default)]
struct PendingToplevel {
    title: String,
    app_id: Option<String>,
    state: ToplevelState,
    outputs: BTreeSet<OutputId>,
    protocol_identifier: Option<String>,
}

impl PendingToplevel {
    fn from_info(info: &ToplevelInfo) -> Self {
        Self {
            title: info.title.clone(),
            app_id: info.app_id.clone(),
            state: info.state,
            outputs: info.outputs.iter().copied().collect(),
            protocol_identifier: info.protocol_identifier.clone(),
        }
    }

    fn into_info(self, id: ToplevelId, source: ToplevelSource) -> ToplevelInfo {
        ToplevelInfo {
            id,
            title: self.title,
            app_id: self.app_id,
            state: self.state,
            outputs: self.outputs.into_iter().collect(),
            source,
            protocol_identifier: self.protocol_identifier,
        }
    }
}

#[derive(Clone)]
enum ProtocolHandle {
    Wlr(ZwlrForeignToplevelHandleV1),
    Ext(ExtForeignToplevelHandleV1),
}

struct ToplevelRecord {
    source: ToplevelSource,
    handle: ProtocolHandle,
    pending: PendingToplevel,
    committed: Option<ToplevelInfo>,
}

impl ToplevelRecord {
    fn new(source: ToplevelSource, handle: ProtocolHandle) -> Self {
        Self {
            source,
            handle,
            pending: PendingToplevel::default(),
            committed: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct ForeignToplevelStore {
    next_id: u64,
    wlr_global: Option<(u32, u32)>,
    ext_global: Option<(u32, u32)>,
    wlr_manager: Option<ZwlrForeignToplevelManagerV1>,
    ext_list: Option<ExtForeignToplevelListV1>,
    records: BTreeMap<ToplevelId, ToplevelRecord>,
    wlr_objects: HashMap<ObjectId, ToplevelId>,
    ext_objects: HashMap<ObjectId, ToplevelId>,
    events: VecDeque<ToplevelEvent>,
}

impl ForeignToplevelStore {
    pub(crate) fn record_wlr_global(&mut self, name: u32, version: u32) {
        self.wlr_global = Some((name, version));
    }

    pub(crate) fn record_ext_global(&mut self, name: u32, version: u32) {
        self.ext_global = Some((name, version));
    }

    pub(crate) fn wlr_global(&self) -> Option<(u32, u32)> {
        self.wlr_global
    }

    pub(crate) fn ext_global(&self) -> Option<(u32, u32)> {
        self.ext_global
    }

    pub(crate) fn has_binding(&self) -> bool {
        self.wlr_manager.is_some() || self.ext_list.is_some()
    }

    pub(crate) fn bind_wlr(&mut self, manager: ZwlrForeignToplevelManagerV1) {
        self.wlr_manager = Some(manager);
    }

    pub(crate) fn bind_ext(&mut self, list: ExtForeignToplevelListV1) {
        self.ext_list = Some(list);
    }

    pub(crate) fn remove_global(&mut self, name: u32) {
        if self.wlr_global.is_some_and(|(global, _)| global == name) {
            self.wlr_global = None;
            self.wlr_manager = None;
            self.clear_source(ToplevelSource::WlrManagement);
        }
        if self.ext_global.is_some_and(|(global, _)| global == name) {
            self.ext_global = None;
            self.ext_list = None;
            self.clear_source(ToplevelSource::ExtList);
        }
    }

    pub(crate) fn manager_finished(&mut self, source: ToplevelSource) {
        match source {
            ToplevelSource::WlrManagement => {
                self.wlr_manager = None;
                self.wlr_global = None;
            }
            ToplevelSource::ExtList => {
                self.ext_list = None;
                self.ext_global = None;
            }
        }
        self.clear_source(source);
    }

    pub(crate) fn capabilities(&self) -> ToplevelCapabilities {
        if let Some(manager) = self.wlr_manager.as_ref() {
            wlr_capabilities(manager.version())
        } else if self.ext_list.is_some() {
            ToplevelCapabilities {
                list: true,
                ..ToplevelCapabilities::default()
            }
        } else {
            ToplevelCapabilities::default()
        }
    }

    pub(crate) fn toplevels(&self) -> Vec<ToplevelInfo> {
        self.records
            .values()
            .filter_map(|record| record.committed.clone())
            .collect()
    }

    pub(crate) fn drain_events(&mut self) -> impl Iterator<Item = ToplevelEvent> + '_ {
        self.events.drain(..)
    }

    pub(crate) fn register_wlr(&mut self, handle: ZwlrForeignToplevelHandleV1) {
        let id = self.allocate_id();
        self.wlr_objects.insert(handle.id(), id);
        self.records.insert(
            id,
            ToplevelRecord::new(ToplevelSource::WlrManagement, ProtocolHandle::Wlr(handle)),
        );
    }

    pub(crate) fn register_ext(&mut self, handle: ExtForeignToplevelHandleV1) {
        let id = self.allocate_id();
        self.ext_objects.insert(handle.id(), id);
        self.records.insert(
            id,
            ToplevelRecord::new(ToplevelSource::ExtList, ProtocolHandle::Ext(handle)),
        );
    }

    pub(crate) fn set_wlr_title(&mut self, object: &ObjectId, title: String) {
        if let Some(record) = self.wlr_record_mut(object) {
            record.pending.title = title;
        }
    }

    pub(crate) fn set_wlr_app_id(&mut self, object: &ObjectId, app_id: String) {
        if let Some(record) = self.wlr_record_mut(object) {
            record.pending.app_id = Some(app_id);
        }
    }

    pub(crate) fn set_wlr_state(&mut self, object: &ObjectId, state: ToplevelState) {
        if let Some(record) = self.wlr_record_mut(object) {
            record.pending.state = state;
        }
    }

    pub(crate) fn wlr_output_enter(&mut self, object: &ObjectId, output: OutputId) {
        if let Some(record) = self.wlr_record_mut(object) {
            record.pending.outputs.insert(output);
        }
    }

    pub(crate) fn wlr_output_leave(&mut self, object: &ObjectId, output: OutputId) {
        if let Some(record) = self.wlr_record_mut(object) {
            record.pending.outputs.remove(&output);
        }
    }

    pub(crate) fn set_ext_title(&mut self, object: &ObjectId, title: String) {
        if let Some(record) = self.ext_record_mut(object) {
            record.pending.title = title;
        }
    }

    pub(crate) fn set_ext_app_id(&mut self, object: &ObjectId, app_id: String) {
        if let Some(record) = self.ext_record_mut(object) {
            record.pending.app_id = Some(app_id);
        }
    }

    pub(crate) fn set_ext_identifier(&mut self, object: &ObjectId, identifier: String) {
        if let Some(record) = self.ext_record_mut(object) {
            record.pending.protocol_identifier = Some(identifier);
        }
    }

    pub(crate) fn commit_wlr(&mut self, object: &ObjectId) {
        if let Some(id) = self.wlr_objects.get(object).copied() {
            self.commit(id);
        }
    }

    pub(crate) fn commit_ext(&mut self, object: &ObjectId) {
        if let Some(id) = self.ext_objects.get(object).copied() {
            self.commit(id);
        }
    }

    pub(crate) fn close_wlr(&mut self, object: &ObjectId) -> Option<ZwlrForeignToplevelHandleV1> {
        let id = self.wlr_objects.remove(object)?;
        let record = self.records.remove(&id)?;
        if record.committed.is_some() {
            self.events.push_back(ToplevelEvent::Removed(id));
        }
        match record.handle {
            ProtocolHandle::Wlr(handle) => Some(handle),
            ProtocolHandle::Ext(_) => None,
        }
    }

    pub(crate) fn close_ext(&mut self, object: &ObjectId) -> Option<ExtForeignToplevelHandleV1> {
        let id = self.ext_objects.remove(object)?;
        let record = self.records.remove(&id)?;
        if record.committed.is_some() {
            self.events.push_back(ToplevelEvent::Removed(id));
        }
        match record.handle {
            ProtocolHandle::Ext(handle) => Some(handle),
            ProtocolHandle::Wlr(_) => None,
        }
    }

    pub(crate) fn wlr_handle(&self, id: ToplevelId) -> Option<ZwlrForeignToplevelHandleV1> {
        match &self.records.get(&id)?.handle {
            ProtocolHandle::Wlr(handle) => Some(handle.clone()),
            ProtocolHandle::Ext(_) => None,
        }
    }

    fn allocate_id(&mut self) -> ToplevelId {
        self.next_id = self.next_id.saturating_add(1).max(1);
        ToplevelId(self.next_id)
    }

    fn commit(&mut self, id: ToplevelId) {
        let Some(record) = self.records.get_mut(&id) else {
            return;
        };
        let info = record.pending.clone().into_info(id, record.source);
        let event = match record.committed.as_ref() {
            None => Some(ToplevelEvent::Added(info.clone())),
            Some(previous) if previous != &info => Some(ToplevelEvent::Changed(info.clone())),
            Some(_) => None,
        };
        record.committed = Some(info.clone());
        record.pending = PendingToplevel::from_info(&info);
        if let Some(event) = event {
            self.events.push_back(event);
        }
    }

    fn wlr_record_mut(&mut self, object: &ObjectId) -> Option<&mut ToplevelRecord> {
        let id = self.wlr_objects.get(object)?;
        self.records.get_mut(id)
    }

    fn ext_record_mut(&mut self, object: &ObjectId) -> Option<&mut ToplevelRecord> {
        let id = self.ext_objects.get(object)?;
        self.records.get_mut(id)
    }

    fn clear_source(&mut self, source: ToplevelSource) {
        let ids: Vec<_> = self
            .records
            .iter()
            .filter_map(|(id, record)| (record.source == source).then_some(*id))
            .collect();
        for id in ids {
            if let Some(record) = self.records.remove(&id) {
                self.wlr_objects.retain(|_, value| *value != id);
                self.ext_objects.retain(|_, value| *value != id);
                if record.committed.is_some() {
                    self.events.push_back(ToplevelEvent::Removed(id));
                }
            }
        }
    }
}

fn wlr_capabilities(version: u32) -> ToplevelCapabilities {
    ToplevelCapabilities {
        list: true,
        state: true,
        activate: true,
        fullscreen: version >= 2,
        close: true,
    }
}

pub(crate) fn parse_wlr_state(bytes: &[u8]) -> ToplevelState {
    let mut state = ToplevelState::default();
    for raw in bytes.chunks_exact(4) {
        let value = u32::from_ne_bytes([raw[0], raw[1], raw[2], raw[3]]);
        match value {
            0 => state.maximized = true,
            1 => state.minimized = true,
            2 => state.activated = true,
            3 => state.fullscreen = true,
            _ => {}
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_ids_round_trip_without_titles_or_app_ids() {
        let id = ToplevelId(0x1234);
        assert_eq!(id.to_string(), "tl:0000000000001234");
        assert_eq!(id.to_string().parse::<ToplevelId>(), Ok(id));
        assert!("foot".parse::<ToplevelId>().is_err());
        assert!("Terminal".parse::<ToplevelId>().is_err());
    }

    #[test]
    fn wlr_fullscreen_capability_requires_protocol_v2() {
        let v1 = wlr_capabilities(1);
        assert!(v1.list);
        assert!(v1.activate);
        assert!(v1.close);
        assert!(!v1.fullscreen);

        let v2 = wlr_capabilities(2);
        assert!(v2.fullscreen);
    }

    #[test]
    fn finished_wlr_manager_forgets_wlr_global_and_preserves_ext_fallback() {
        let mut store = ForeignToplevelStore::default();
        store.record_wlr_global(7, 3);
        store.record_ext_global(9, 1);

        store.manager_finished(ToplevelSource::WlrManagement);

        assert_eq!(store.wlr_global(), None);
        assert_eq!(store.ext_global(), Some((9, 1)));
        assert!(!store.has_binding());
    }

    #[test]
    fn finished_ext_list_forgets_ext_global() {
        let mut store = ForeignToplevelStore::default();
        store.record_ext_global(9, 1);

        store.manager_finished(ToplevelSource::ExtList);

        assert_eq!(store.ext_global(), None);
        assert!(!store.has_binding());
    }

    #[test]
    fn wlr_state_array_is_decoded_atomically() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&2_u32.to_ne_bytes());
        bytes.extend_from_slice(&3_u32.to_ne_bytes());
        let state = parse_wlr_state(&bytes);
        assert!(state.activated);
        assert!(state.fullscreen);
        assert!(!state.maximized);
        assert!(!state.minimized);
    }
}
