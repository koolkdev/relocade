//! Capturing, admitting and publishing runtime-owned snapshot groups.

mod cache;
use crate::{DispatchEntry, Result, Runtime, RuntimeBackend, RuntimeState};
pub(crate) use cache::CodeCache;
use cache::{CodeState, Group};
use relocade_x86::{BlockSnapshot, CompiledBlocks, Compiler, PhysicalMapping, PhysicalMemoryMap};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, Weak},
};

/// One CS-relative entry to capture from the stopped guest.
#[derive(Clone, Copy, Debug)]
pub struct BlockRequest {
    pub start_eip: u32,
    pub instruction_limit: u32,
}

/// Identity of one published group in its owning runtime.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub struct GroupId(u64);

/// Owned compilation inputs. Only the runtime that captured them can publish
/// them, and intervening writes or entry changes make them stale.
pub struct PreparedGroup {
    owner: Weak<Mutex<CodeState>>,
    generation: u64,
    module: CompiledBlocks,
    snapshots: Vec<Vec<u8>>,
    group: Group,
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Captures readable RAM/ROM under the current CS and compiles a linked group.
    /// Returns None for an empty request, unavailable code, a fixed host entry,
    /// or an instruction sequence the snapshot compiler cannot translate.
    /// The runtime owns byte validity after publication, including backing aliases.
    pub fn prepare_group(&mut self, requests: &[BlockRequest]) -> Result<Option<PreparedGroup>> {
        if requests.is_empty() {
            return Ok(None);
        }
        let cpu = self.machine().cpu().snapshot();
        let base = cpu.segments.cs.base;
        let cache = self.storage.code.as_ref().unwrap();
        let state = cache.state.lock().unwrap();
        let generation = state.generation;
        let boundaries = state.boundaries();
        let mut snapshots = Vec::with_capacity(requests.len());
        for request in requests {
            if request.start_eip > cpu.segments.cs.limit
                || request.instruction_limit == 0
                || state.fixed.contains(&base.wrapping_add(request.start_eip))
            {
                return Ok(None);
            }
            let max = (u64::from(cpu.segments.cs.limit) + 1 - u64::from(request.start_eip))
                .min(u64::from(request.instruction_limit) * 15) as usize;
            let mut bytes = Vec::with_capacity(max);
            for index in 0..max {
                let address = base
                    .wrapping_add(request.start_eip)
                    .wrapping_add(index as u32);
                if index != 0 && boundaries.contains(&address) {
                    break;
                }
                let backing_offset = match self.config.memory_map().get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => backing_offset,
                    _ => break,
                };
                let mut byte = [0];
                self.storage
                    .access
                    .read_memory(&self.storage.guest, backing_offset, &mut byte);
                bytes.push(byte[0]);
            }
            snapshots.push(bytes);
        }
        let target = self.config.execution_target();
        let compiler = Compiler::new(target.profile)
            .with_cpu_model(target.cpu_model)
            .with_code_base(base)
            .with_block_boundaries(boundaries.iter().map(|address| address.wrapping_sub(base)));
        let inputs: Vec<_> = requests
            .iter()
            .zip(&snapshots)
            .map(|(request, bytes)| BlockSnapshot {
                start_eip: request.start_eip,
                bytes,
                instruction_limit: request.instruction_limit,
            })
            .collect();
        let module = match compiler.compile_blocks(&inputs) {
            Ok(module) => module,
            Err(relocade_x86::BlockError::Compiler(error)) => return Err(error.into()),
            Err(_) => return Ok(None),
        };
        let mut pages = BTreeSet::new();
        let mut spans = Vec::new();
        for entry in &module.entries {
            let start = base + entry.start_eip;
            let end = start + entry.byte_len as u32;
            spans.push(start..end);
            for address in start..end {
                let backing_offset = match self.config.memory_map().get(address) {
                    PhysicalMapping::Ram { backing_offset }
                    | PhysicalMapping::Rom { backing_offset } => backing_offset,
                    _ => unreachable!("compiled bytes were captured from backing"),
                };
                pages.insert(backing_offset / PhysicalMemoryMap::PAGE_BYTES);
            }
        }
        for (snapshot, entry) in snapshots.iter_mut().zip(&module.entries) {
            snapshot.truncate(entry.byte_len);
        }
        Ok(Some(PreparedGroup {
            owner: Arc::downgrade(&cache.state),
            generation,
            module,
            snapshots,
            group: Group { spans, pages },
        }))
    }

    /// Compiles/links in the engine and publishes only if the captured generation
    /// and instruction bytes still belong to this runtime. Stale inputs return
    /// None before publication.
    /// External exports guard CS.base/EIP; local links bypass those admission guards.
    pub async fn install_group(&mut self, prepared: PreparedGroup) -> Result<Option<GroupId>> {
        if !self.group_is_current(&prepared) {
            return Ok(None);
        }
        let instance = self.instantiate(&prepared.module.bytes, &[]).await?;
        if !self.group_is_current(&prepared) {
            return Ok(None);
        }
        let entries: Vec<_> = prepared
            .module
            .entries
            .iter()
            .zip(&prepared.group.spans)
            .map(|(entry, span)| DispatchEntry {
                address: span.start,
                export: &entry.export,
            })
            .collect();
        let cache = self.storage.code.as_ref().unwrap();
        let addresses: Vec<_> = entries.iter().map(|entry| entry.address).collect();
        cache.before_entries(&mut self.storage.access, &addresses);
        self.dispatch_table
            .install(&mut self.storage.access, &instance, &entries)?;
        Ok(Some(
            cache.install(&mut self.storage.access, prepared.group),
        ))
    }

    fn group_is_current(&self, prepared: &PreparedGroup) -> bool {
        let cache = self.storage.code.as_ref().unwrap();
        if !prepared.owner.ptr_eq(&Arc::downgrade(&cache.state))
            || cache.state.lock().unwrap().generation != prepared.generation
        {
            return false;
        }
        // Unpublished snapshots are not watched. Guest execution between capture
        // and installation may have changed them through an ordinary RAM store.
        prepared
            .group
            .spans
            .iter()
            .zip(&prepared.snapshots)
            .all(|(span, bytes)| {
                span.clone().zip(bytes).all(|(address, &expected)| {
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
}
