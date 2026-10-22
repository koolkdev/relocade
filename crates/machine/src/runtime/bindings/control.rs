//! Execution controls and code-version checks for bound resources.

use crate::{memory::version, CpuBindings, MemoryVersion};
use relocade_wasm_runtime::WasmMemoryAccess;
use relocade_x86::ExecutionContext;
use std::mem::offset_of;

impl<M> CpuBindings<M> {
    pub(crate) fn set_instruction_target(
        &self,
        backend: &mut impl WasmMemoryAccess<Memory = M>,
        target: u64,
    ) {
        backend.write_memory(
            &self.context,
            offset_of!(ExecutionContext, instruction_target) as u32,
            &target.to_le_bytes(),
        );
    }

    pub(crate) fn mapping_generation(&self, backend: &impl WasmMemoryAccess<Memory = M>) -> u64 {
        let mut bytes = [0; version::BYTE_LEN as usize];
        backend.read_memory(&self.revision, 0, &mut bytes);
        u64::from_le_bytes(bytes)
    }

    pub(crate) fn matches_version(
        &self,
        backend: &impl WasmMemoryAccess<Memory = M>,
        version: &MemoryVersion,
    ) -> bool {
        version.matches(&self.identity, self.mapping_generation(backend))
    }
}
