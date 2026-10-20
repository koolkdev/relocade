use crate::wasm::WasmMemoryAccess;
use relocade_machine::MachineStorage;
use relocade_x86::ExecutionContext;
use std::mem::offset_of;

/// Routes CPU and guest transfers through a backend or its scoped context.
pub(crate) struct Storage<A: WasmMemoryAccess> {
    pub access: A,
    pub context: A::Memory,
    pub guest: A::Memory,
}

impl<A: WasmMemoryAccess> Storage<A> {
    pub fn set_instruction_target(&mut self, target: u64) {
        self.access.write_memory(
            &self.context,
            offset_of!(ExecutionContext, instruction_target) as u32,
            &target.to_le_bytes(),
        );
    }
}

impl<A: WasmMemoryAccess> MachineStorage for Storage<A> {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]) {
        self.access.read_memory(
            &self.context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }

    fn write_cpu(&mut self, offset: u32, bytes: &[u8]) {
        self.access.write_memory(
            &self.context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }

    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        self.access.read_memory(&self.guest, offset, bytes);
    }

    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.access.write_memory(&self.guest, offset, bytes);
    }
}
