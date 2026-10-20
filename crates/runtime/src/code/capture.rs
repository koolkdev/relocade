//! Owned code capture. No decoding or compilation runs while reading the guest.

use super::{cache::CodeState, compile::CompilationInput, BlockRequest, PreparedGroup};
use crate::{Result, Runtime, RuntimeBackend, RuntimeState};
use relocade_x86::{PhysicalMapping, PhysicalMemoryMap};
use std::sync::{Arc, Mutex, Weak};

/// A bounded copy of readable code and its compilation policy. It owns no live
/// guest memory or engine handles and can be moved to a compiler thread.
pub struct CodeSnapshot {
    pub(super) owner: Weak<Mutex<CodeState>>,
    pub(super) input: CompilationInput,
}

impl CodeSnapshot {
    /// Discovers and lowers blocks using only the captured bytes.
    pub fn compile(self) -> Result<Option<PreparedGroup>> {
        Ok(self.input.compile()?.map(|code| PreparedGroup {
            owner: self.owner,
            code,
        }))
    }
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Runtime<B> {
    /// Copies the current real-mode code segment, including readable RAM/ROM
    /// aliases. Holes and MMIO are not read. Capture is bounded by 64 KiB and
    /// does not mark pending code as watched or perform any compiler work.
    pub fn capture_group(&mut self, requests: &[BlockRequest]) -> Option<CodeSnapshot> {
        if requests.is_empty()
            || requests
                .iter()
                .any(|request| request.instruction_limit == 0)
        {
            return None;
        }
        let mut snapshot = self.capture_code()?;
        if requests
            .iter()
            .any(|request| !snapshot.input.can_start(request.start_eip))
        {
            return None;
        }
        snapshot.input.requests = requests.to_vec();
        Some(snapshot)
    }

    /// Captures a connected-group request without discovering its successors.
    /// Discovery and lowering happen when the owned snapshot is compiled.
    pub fn capture_connected_group(
        &mut self,
        start_eip: u32,
        max_blocks: std::num::NonZeroU16,
        instructions_per_block: std::num::NonZeroU16,
    ) -> Option<CodeSnapshot> {
        let mut snapshot = self.capture_group(&[BlockRequest {
            start_eip,
            instruction_limit: u32::from(instructions_per_block.get()),
        }])?;
        snapshot.input.max_blocks = usize::from(max_blocks.get());
        Some(snapshot)
    }

    fn capture_code(&mut self) -> Option<CodeSnapshot> {
        let cpu = self.machine().cpu().snapshot();
        let target = self.config.execution_target();
        if !target.profile.is_compatible_with(&cpu.segments) {
            return None;
        }
        let base = cpu.segments.cs.base;
        let cache = self.storage.code.as_ref().unwrap();
        let state = cache.state.lock().unwrap();
        let mut input = CompilationInput {
            target,
            base,
            boundaries: state
                .boundaries()
                .into_iter()
                .map(|address| address.wrapping_sub(base))
                .collect(),
            fixed: state
                .fixed
                .iter()
                .map(|address| address.wrapping_sub(base))
                .collect(),
            regions: Vec::new(),
            requests: Vec::new(),
            max_blocks: 0,
        };
        drop(state);
        // MachineConfig currently admits canonical Real16 caches only.
        let mut eip = 0;
        while eip <= cpu.segments.cs.limit {
            let address = base.checked_add(eip)?;
            let len = (PhysicalMemoryMap::PAGE_BYTES - address % PhysicalMemoryMap::PAGE_BYTES)
                .min(cpu.segments.cs.limit - eip + 1);
            if let PhysicalMapping::Ram { backing_offset }
            | PhysicalMapping::Rom { backing_offset } = self.config.memory_map().get(address)
            {
                let mut bytes = vec![0; len as usize];
                self.storage
                    .access
                    .read_memory(&self.storage.guest, backing_offset, &mut bytes);
                input.append(eip, bytes);
            }
            eip += len;
        }
        Some(CodeSnapshot {
            owner: Arc::downgrade(&cache.state),
            input,
        })
    }
}
