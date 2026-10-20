//! Capturing, admitting and publishing runtime-owned snapshot groups.

mod cache;
mod capture;
mod compile;
#[cfg(feature = "compiler-wire")]
mod wire;
use crate::{DispatchEntry, Result, Runtime, RuntimeBackend, RuntimeState};
pub(crate) use cache::CodeCache;
use cache::{CodeState, Group};
pub use capture::CodeSnapshot;
use compile::CompiledCode;
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, Weak},
};

/// One CS-relative entry to capture from the stopped guest.
#[derive(Clone, Copy, Debug)]
#[cfg_attr(
    feature = "compiler-wire",
    derive(serde::Serialize, serde::Deserialize)
)]
pub struct BlockRequest {
    pub start_eip: u32,
    pub instruction_limit: u32,
}

/// Identity of one published group in its owning runtime.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GroupId(u64);

/// Lowered code and the exact instruction bytes it consumed. Publication checks
/// ownership, consumed bytes and fixed entry boundaries again.
pub struct PreparedGroup {
    owner: Weak<Mutex<CodeState>>,
    code: CompiledCode,
}

impl PreparedGroup {
    /// Wasm bytes for engine compilation. No guest imports are bound yet.
    pub fn wasm(&self) -> &[u8] {
        &self.code.module.bytes
    }
}

/// A prepared group coupled to the engine module compiled from its Wasm bytes.
/// It contains no guest instance and is published only by the capturing runtime.
pub struct CompiledGroup<M> {
    prepared: PreparedGroup,
    module: M,
}

impl<M> CompiledGroup<M> {
    /// Backends must supply the module compiled from `prepared.wasm()` using a
    /// compatible engine. Instantiation stays on the runtime's owning thread.
    pub fn new(prepared: PreparedGroup, module: M) -> Self {
        Self { prepared, module }
    }
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Captures readable RAM/ROM under the current CS and compiles a linked group.
    /// Returns None for an empty request, unavailable code, a fixed host entry,
    /// or an instruction sequence the snapshot compiler cannot translate.
    /// The runtime owns byte validity after publication, including backing aliases.
    pub fn prepare_group(&mut self, requests: &[BlockRequest]) -> Result<Option<PreparedGroup>> {
        match self.capture_group(requests) {
            Some(snapshot) => snapshot.compile(),
            None => Ok(None),
        }
    }

    /// Compiles in the engine and publishes a manually prepared group. Explicit
    /// preparation may wait for compilation; automatic background work instead
    /// uses [`Runtime::publish_group`] with an already compiled engine module.
    pub async fn install_group(&mut self, prepared: PreparedGroup) -> Result<Option<GroupId>> {
        if !self.group_is_current(&prepared) {
            return Ok(None);
        }
        let module = self.storage.access.compile(prepared.wasm()).await?;
        self.publish_group(CompiledGroup::new(prepared, module))
            .await
    }

    /// Instantiates already compiled code with this guest's private imports and
    /// publishes it while execution is stopped. Stale results return None.
    /// External exports guard CS.base/EIP; local links bypass those admission guards.
    pub async fn publish_group(
        &mut self,
        compiled: CompiledGroup<B::Module>,
    ) -> Result<Option<GroupId>> {
        let prepared = compiled.prepared;
        if !self.group_is_current(&prepared) {
            return Ok(None);
        }
        let instance = self.instantiate_module(&compiled.module, &[]).await?;
        if !self.group_is_current(&prepared) {
            return Ok(None);
        }
        let mut group = Group {
            spans: Vec::new(),
            pages: BTreeSet::new(),
        };
        for entry in &prepared.code.module.entries {
            let start = prepared.code.base + entry.start_eip;
            let end = start + entry.byte_len as u32;
            group.spans.push(start..end);
            for address in start..end {
                let backing = match self.config.memory_map().get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => backing_offset,
                    _ => unreachable!("publication validated readable code"),
                };
                group.pages.insert(backing / PhysicalMemoryMap::PAGE_BYTES);
            }
        }
        let entries: Vec<_> = prepared
            .code
            .module
            .entries
            .iter()
            .zip(&group.spans)
            .map(|(entry, span)| DispatchEntry {
                address: span.start,
                export: &entry.export,
            })
            .collect();
        let cache = self.storage.code.as_ref().unwrap();
        let addresses: Vec<_> = entries.iter().map(|entry| entry.address).collect();
        cache.before_entries(&mut self.storage.access, &addresses);
        cache.trim(
            &mut self.storage.access,
            usize::from(self.jit.config.max_groups.get()) - 1,
        );
        self.dispatch_table
            .reclaim(&mut self.storage.access, &self.interpret)?;
        self.dispatch_table
            .install(&mut self.storage.access, &instance, &entries)?;
        Ok(Some(cache.install(&mut self.storage.access, group)))
    }

    fn group_is_current(&self, prepared: &PreparedGroup) -> bool {
        let cache = self.storage.code.as_ref().unwrap();
        if !prepared.owner.ptr_eq(&Arc::downgrade(&cache.state))
            || prepared.code.module.execution_target != self.config.execution_target()
        {
            return false;
        }
        let state = cache.state.lock().unwrap();
        // Pending snapshots are unwatched: ordinary guest stores keep their
        // existing fast path. Only consumed bytes and fixed entry boundaries
        // affect admission; unrelated writes and managed publications do not.
        prepared
            .code
            .module
            .entries
            .iter()
            .zip(&prepared.code.snapshots)
            .all(|(entry, bytes)| {
                let start = prepared.code.base + entry.start_eip;
                let end = start + entry.byte_len as u32;
                state.fixed.range(start..end).next().is_none()
                    && (start..end).zip(bytes).all(|(address, &expected)| {
                        let backing = match self.config.memory_map().get(address) {
                            PhysicalMapping::Ram { backing_offset }
                            | PhysicalMapping::Rom { backing_offset } => backing_offset,
                            _ => return false,
                        };
                        let mut current = [0];
                        self.storage
                            .access
                            .read_memory(&self.storage.guest, backing, &mut current);
                        current[0] == expected
                    })
            })
    }

    pub(crate) fn publish_fixed(
        &mut self,
        instance: &B::Instance,
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let cache = self.storage.code.as_ref().unwrap();
        let addresses: Vec<_> = entries.iter().map(|entry| entry.address).collect();
        cache.before_entries(&mut self.storage.access, &addresses);
        self.dispatch_table
            .reclaim(&mut self.storage.access, &self.interpret)?;
        self.dispatch_table
            .install(&mut self.storage.access, instance, entries)?;
        cache.state.lock().unwrap().fixed.extend(addresses);
        Ok(())
    }

    /// Number of currently callable managed groups (retired groups are excluded).
    pub fn compiled_group_count(&self) -> usize {
        self.storage
            .code
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap()
            .groups
            .len()
    }

    /// Number of callable managed entries, excluding fixed modules and hooks.
    pub fn compiled_entry_count(&self) -> usize {
        self.storage
            .code
            .as_ref()
            .unwrap()
            .state
            .lock()
            .unwrap()
            .groups
            .values()
            .map(|group| group.spans.len())
            .sum()
    }
}
