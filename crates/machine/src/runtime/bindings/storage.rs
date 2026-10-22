//! Byte transfers through a CPU's bound Wasm memories.

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

    pub(crate) fn read_backing(
        &self,
        backend: &impl WasmMemoryAccess<Memory = M>,
        offset: u32,
        bytes: &mut [u8],
    ) {
        backend.read_memory(&self.guest, offset, bytes);
    }

    pub(crate) fn write_backing(
        &self,
        backend: &mut impl WasmMemoryAccess<Memory = M>,
        offset: u32,
        bytes: &[u8],
    ) {
        backend.write_memory(&self.guest, offset, bytes);
    }

    pub(crate) fn read_cpu(
        &self,
        backend: &impl WasmMemoryAccess<Memory = M>,
        offset: u32,
        bytes: &mut [u8],
    ) {
        backend.read_memory(
            &self.context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }

    pub(crate) fn write_cpu(
        &self,
        backend: &mut impl WasmMemoryAccess<Memory = M>,
        offset: u32,
        bytes: &[u8],
    ) {
        backend.write_memory(
            &self.context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }
}
