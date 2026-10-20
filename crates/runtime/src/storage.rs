use crate::{
    wasm::{Import, WasmBackend, WasmMemoryAccess},
    Result,
};
use relocade_machine::{DescriptorMemory, MachineConfig, MachineStorage, MemoryMap};
use relocade_x86::{abi, ExecutionContext};
use std::{mem::offset_of, sync::Arc};

/// The initialized Wasm memories for one machine. This owner allocates the CPU,
/// guest backing, translation map and dedicated descriptor storage together,
/// and supplies their bindings and scoped host access.
#[derive(Clone)]
pub struct MachineMemory<M> {
    pub(crate) config: Arc<MachineConfig>,
    pub(crate) context: M,
    pub(crate) guest: M,
    pub(crate) mapping: M,
    pub(crate) descriptors: M,
}

impl<M: Clone + Send + Sync + 'static> MachineMemory<M> {
    pub fn new<B: WasmBackend<Memory = M>>(backend: &mut B, config: MachineConfig) -> Result<Self> {
        let context = backend.create_memory(1, Some(1))?;
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
        let guest = backend.create_memory(guest_pages, None)?;
        let mapping_bytes = config.memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = backend.create_memory(mapping_pages, Some(mapping_pages))?;
        backend.write_memory(&mapping, 0, &mapping_bytes);
        let descriptor_pages = DescriptorMemory::BYTE_LEN / 65536;
        let descriptors = backend.create_memory(descriptor_pages, Some(descriptor_pages))?;
        backend.write_memory(&descriptors, 0, config.descriptor_memory().to_bytes());
        Ok(Self {
            config: Arc::new(config),
            context,
            guest,
            mapping,
            descriptors,
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

    /// Binds the CPU, guest backing and profile-specific translation memory.
    /// Descriptor storage is accessed only by the runtime's descriptor callbacks.
    pub fn imports<'a, B: WasmBackend<Memory = M> + 'a>(
        &'a self,
        backend: &B,
    ) -> [Import<'a, B>; 3] {
        let mapping = match self.config.memory_map() {
            MemoryMap::Physical(_) => abi::PHYSICAL_MAP,
            MemoryMap::Paged(_) => abi::PAGE_TABLE,
        };
        [
            backend.bind(abi::EXECUTION_CONTEXT, &self.context),
            backend.bind(abi::GUEST, &self.guest),
            backend.bind(mapping, &self.mapping),
        ]
    }
}

/// Routes transfers through a backend or its scoped context, without caching CPU state.
pub(crate) struct Storage<A: WasmMemoryAccess> {
    pub access: A,
    pub memory: MachineMemory<A::Memory>,
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
    fn read_mapping(&self, offset: u32, bytes: &mut [u8]) {
        self.access.read_memory(&self.memory.mapping, offset, bytes);
    }
    fn write_mapping(&mut self, offset: u32, bytes: &[u8]) {
        self.access
            .write_memory(&self.memory.mapping, offset, bytes);
    }
    fn read_descriptors(&self, offset: u32, bytes: &mut [u8]) {
        self.access
            .read_memory(&self.memory.descriptors, offset, bytes);
    }
    fn write_descriptors(&mut self, offset: u32, bytes: &[u8]) {
        self.access
            .write_memory(&self.memory.descriptors, offset, bytes);
    }
}
