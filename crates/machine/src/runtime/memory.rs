//! Wasm memory resources and scoped access for runtime execution.

use super::{Result, RuntimeState};
use crate::{CpuConfig, CpuContext, MachineConfig};
use relocade_compiler::abi::Memory as MemoryDeclaration;
use relocade_wasm_runtime::{
    Import, Memory, SharedMemory, WasmBackend, WasmContext, WasmMemoryAccess, WasmSharedMemory,
};
use relocade_x86::{abi, ExecutionContext};
use std::mem::offset_of;

/// The runtime's initialized Wasm memories. This owner allocates backend memory
/// from the machine configuration and supplies module bindings. The CPU's
/// callbacks retain these resources and borrow access when the guest is stopped.
pub(super) struct RuntimeMemory {
    pub(crate) config: CpuConfig,
    pub(crate) machine_config: MachineConfig,
    pub(crate) context: Memory,
    pub(crate) guest: Memory,
    pub(crate) mapping: Memory,
    pub(crate) shared_guest: SharedMemory,
}

impl RuntimeMemory {
    pub fn new<T: 'static>(
        backend: &mut super::Backend<T>,
        machine_config: MachineConfig,
        config: CpuConfig,
    ) -> Result<Self> {
        machine_config.validate_for(config.execution_target().profile)?;
        let context = backend.create_memory(MemoryDeclaration {
            maximum: Some(1),
            ..abi::EXECUTION_CONTEXT
        })?;
        backend.memory(&context).write(
            0,
            &ExecutionContext {
                cpu: *config.cpu(),
                cpu_model: config.execution_target().cpu_model,
                ..ExecutionContext::default()
            }
            .to_bytes(),
        );
        let guest_pages = machine_config.memory_size().div_ceil(65536).max(1) as u32;
        let guest = backend.create_memory(MemoryDeclaration {
            minimum: guest_pages,
            ..abi::GUEST
        })?;
        let mapping_bytes = machine_config.memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = backend.create_memory(MemoryDeclaration {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        backend.memory(&mapping).write(0, &mapping_bytes);
        let shared_guest = backend
            .shared_memory(&guest)
            .expect("guest backing is shared");
        Ok(Self {
            config,
            machine_config,
            context,
            guest,
            mapping,
            shared_guest,
        })
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }
    /// Binds the CPU, guest backing and physical translation memory.
    pub fn imports<'a, B: WasmBackend<Memory = Memory> + 'a>(
        &'a self,
        backend: &B,
    ) -> [Import<'a, B>; 3] {
        [
            backend.bind(abi::EXECUTION_CONTEXT, &self.context),
            backend.bind(abi::GUEST, &self.guest),
            backend.bind(abi::PHYSICAL_MAP, &self.mapping),
        ]
    }

    pub(crate) fn context<'a, T: 'static>(
        &'a self,
        access: &'a mut impl WasmContext<State = RuntimeState<T>, Memory = Memory>,
    ) -> CpuContext<'a, T> {
        let (context, state) = access.memory_and_state(&self.context);
        CpuContext::new(
            &self.config,
            &self.machine_config,
            context,
            self.shared_guest.view(),
            state,
        )
    }

    pub(crate) fn set_instruction_target(
        &self,
        access: &mut impl WasmMemoryAccess<Memory = Memory>,
        target: u64,
    ) {
        access.memory(&self.context).write(
            offset_of!(ExecutionContext, instruction_target) as u32,
            &target.to_le_bytes(),
        );
    }
}
