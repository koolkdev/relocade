//! Execution controls and code-version checks for bound resources.

use crate::CpuBindings;
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
}
