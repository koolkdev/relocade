//! Code dependency retirement, shared by stopped writes and guest slow writes.

use super::GroupId;
use crate::{dispatch::Entries, wasm::WasmMemoryAccess};
use relocade_x86::PhysicalMemoryMap;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
    sync::{Arc, Mutex},
};

pub(super) struct Group {
    pub spans: Vec<Range<u32>>,
    pub pages: BTreeSet<u32>,
}

pub(crate) struct CodeState {
    pub generation: u64,
    pub fixed: BTreeSet<u32>,
    pub(super) groups: BTreeMap<GroupId, Group>,
    map: PhysicalMemoryMap,
    page_users: BTreeMap<u32, usize>,
    next_id: u64,
}

#[derive(Clone)]
pub(crate) struct CodeCache<M> {
    pub state: Arc<Mutex<CodeState>>,
    entries: Arc<Mutex<Entries>>,
    entry_memory: M,
    mapping: M,
}

impl<M: Clone> CodeCache<M> {
    pub fn new(
        map: PhysicalMemoryMap,
        entries: Arc<Mutex<Entries>>,
        entry_memory: M,
        mapping: M,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(CodeState {
                generation: 0,
                fixed: BTreeSet::new(),
                groups: BTreeMap::new(),
                map,
                page_users: BTreeMap::new(),
                next_id: 0,
            })),
            entries,
            entry_memory,
            mapping,
        }
    }

    pub fn before_write(
        &self,
        access: &mut impl WasmMemoryAccess<Memory = M>,
        offset: u32,
        len: usize,
    ) {
        if len == 0 {
            return;
        }
        let first = offset / PhysicalMemoryMap::PAGE_BYTES;
        let last = ((u64::from(offset) + len as u64 - 1) / u64::from(PhysicalMemoryMap::PAGE_BYTES))
            as u32;
        let mut state = self.state.lock().unwrap();
        state.advance();
        self.retire_matching(access, &mut state, |group| {
            group.pages.range(first..=last).next().is_some()
        });
    }

    /// New callable boundaries supersede any captured path through that address.
    pub fn before_entries(
        &self,
        access: &mut impl WasmMemoryAccess<Memory = M>,
        addresses: &[u32],
    ) {
        let mut state = self.state.lock().unwrap();
        state.advance();
        self.retire_matching(access, &mut state, |group| {
            addresses
                .iter()
                .any(|address| group.spans.iter().any(|span| span.contains(address)))
        });
    }

    fn retire_matching(
        &self,
        access: &mut impl WasmMemoryAccess<Memory = M>,
        state: &mut CodeState,
        predicate: impl Fn(&Group) -> bool,
    ) {
        let ids: Vec<_> = state
            .groups
            .iter()
            .filter(|(_, group)| predicate(group))
            .map(|(&id, _)| id)
            .collect();
        if ids.is_empty() {
            return;
        }
        let mut entries = self.entries.lock().unwrap();
        for id in ids {
            let group = state.groups.remove(&id).unwrap();
            for span in group.spans {
                entries.remove(span.start);
            }
            for page in group.pages {
                let users = state.page_users.get_mut(&page).unwrap();
                *users -= 1;
                if *users == 0 {
                    state.page_users.remove(&page);
                    state
                        .map
                        .set_code_watched(page * PhysicalMemoryMap::PAGE_BYTES, false);
                }
            }
        }
        // Removal cannot grow the entry image, so callbacks need only memory access.
        access.write_memory(&self.entry_memory, 0, &entries.addresses.to_bytes());
        access.write_memory(&self.mapping, 0, &state.map.to_bytes());
    }

    pub(super) fn install(
        &self,
        access: &mut impl WasmMemoryAccess<Memory = M>,
        group: Group,
    ) -> GroupId {
        let mut state = self.state.lock().unwrap();
        state.advance();
        state.next_id = state
            .next_id
            .checked_add(1)
            .expect("group identity capacity");
        let id = GroupId(state.next_id);
        for &page in &group.pages {
            *state.page_users.entry(page).or_default() += 1;
            state
                .map
                .set_code_watched(page * PhysicalMemoryMap::PAGE_BYTES, true);
        }
        state.groups.insert(id, group);
        access.write_memory(&self.mapping, 0, &state.map.to_bytes());
        id
    }
}

impl CodeState {
    fn advance(&mut self) {
        self.generation = self
            .generation
            .checked_add(1)
            .expect("code generation capacity");
    }

    pub fn boundaries(&self) -> BTreeSet<u32> {
        self.fixed
            .iter()
            .copied()
            .chain(
                self.groups
                    .values()
                    .flat_map(|group| group.spans.iter().map(|span| span.start)),
            )
            .collect()
    }
}
