//! Wasm memory resources and scoped access for runtime execution.

mod shared;
pub use shared::SharedMemory;

use crate::{
    wasm::{Import, WasmBackend, WasmMemoryAccess},
    Result,
};
use relocade_compiler::abi::Memory;
use relocade_machine::{MachineConfig, MachineStorage};
use relocade_x86::{abi, ExecutionContext};
use std::{mem::offset_of, sync::Arc};

/// The runtime's initialized Wasm memories. This owner allocates backend memory
/// from the machine configuration and supplies module bindings and scoped host
/// access. Architectural access policy belongs to `relocade_machine`.
#[derive(Clone)]
pub struct RuntimeMemory<M> {
    pub(crate) config: Arc<MachineConfig>,
    pub(crate) context: M,
    pub(crate) guest: M,
    pub(crate) mapping: M,
}

impl<M: Clone + Send + Sync + 'static> RuntimeMemory<M> {
    pub fn new<B: WasmBackend<Memory = M>>(backend: &mut B, config: MachineConfig) -> Result<Self> {
        let context = backend.create_memory(Memory {
            maximum: Some(1),
            ..abi::EXECUTION_CONTEXT
        })?;
        backend.write_memory(
            &context,
            0,
            &ExecutionContext {
                cpu: *config.cpu(),
                cpu_model: config.execution_target().cpu_model,
                ..ExecutionContext::default()
            }
            .to_bytes(),
        );
        let guest_pages = config.memory_size().div_ceil(65536).max(1) as u32;
        let guest = backend.create_memory(Memory {
            minimum: guest_pages,
            ..abi::GUEST
        })?;
        let mapping_bytes = config.memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = backend.create_memory(Memory {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        backend.write_memory(&mapping, 0, &mapping_bytes);
        Ok(Self {
            config: Arc::new(config),
            context,
            guest,
            mapping,
        })
    }

    pub fn configuration(&self) -> &MachineConfig {
        &self.config
    }
    pub fn context_memory(&self) -> &M {
        &self.context
    }
    pub fn guest_memory(&self) -> &M {
        &self.guest
    }

    /// Binds the CPU, guest backing and physical translation memory.
    pub fn imports<'a, B: WasmBackend<Memory = M> + 'a>(
        &'a self,
        backend: &B,
    ) -> [Import<'a, B>; 3] {
        [
            backend.bind(abi::EXECUTION_CONTEXT, &self.context),
            backend.bind(abi::GUEST, &self.guest),
            backend.bind(abi::PHYSICAL_MAP, &self.mapping),
        ]
    }
}

/// Routes transfers through a backend or its scoped context, without caching CPU state.
pub(crate) struct Storage<A: WasmMemoryAccess> {
    pub access: A,
    pub memory: RuntimeMemory<A::Memory>,
}

impl<A: WasmMemoryAccess> Storage<A> {
    pub fn set_instruction_target(&mut self, target: u64) {
        self.access.write_memory(
            &self.memory.context,
            offset_of!(ExecutionContext, instruction_target) as u32,
            &target.to_le_bytes(),
        );
    }
}

impl<A: WasmMemoryAccess> MachineStorage for Storage<A> {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]) {
        self.access.read_memory(
            &self.memory.context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]) {
        self.access.write_memory(
            &self.memory.context,
            offset_of!(ExecutionContext, cpu) as u32 + offset,
            bytes,
        );
    }
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        self.access.read_memory(&self.memory.guest, offset, bytes);
    }
    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.access.write_memory(&self.memory.guest, offset, bytes);
    }
}
