use crate::memory::resources::MemoryResources;
use relocade_wasm_runtime::WasmMemoryAccess;
mod control;

use crate::Result;
use crate::{cpu::Membership, CpuConfig, CpuId, Machine, MachineConfig, MemoryMap};
use relocade_compiler::abi::Memory;
use relocade_wasm_runtime::{Import, WasmBackend};
use relocade_x86::{abi, ExecutionContext};
use std::sync::Arc;

/// One CPU's context and bindings to guest memory. The context is private;
/// guest backing belongs to the independently owned memory handle.
/// Architectural access policy belongs to `relocade_machine`.
#[derive(Clone)]
pub struct CpuBindings<M> {
    pub(crate) config: Arc<CpuConfig>,
    pub(crate) machine_config: Arc<MachineConfig>,
    pub(crate) membership: Arc<Membership>,
    pub(crate) context: M,
    pub(crate) guest: M,
    pub(crate) mapping: M,
    pub(crate) revision: M,
    pub(crate) identity: Arc<()>,
}

impl<M: Clone + Send + Sync + 'static> CpuBindings<M> {
    pub(crate) fn new<B: WasmBackend<Memory = M>>(
        backend: &mut B,
        config: CpuConfig,
        machine: &Machine<B::SharedMemory>,
    ) -> Result<Self> {
        let memory = &machine.memory;
        memory
            .configuration()
            .validate_for(config.execution_target().profile)?;
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
        let guest = backend.import_shared_memory(memory.backing());
        let revision = backend.import_shared_memory(&memory.resources.revision);
        let mapping = match memory.configuration().memory_map() {
            MemoryMap::Paged(_) => {
                backend.import_shared_memory(memory.resources.mapping.as_ref().expect("page table"))
            }
            MemoryMap::Physical(map) => {
                let bytes = map.to_bytes();
                let pages = bytes.len().div_ceil(65536).max(1) as u32;
                let mapping = backend.create_memory(Memory {
                    minimum: pages,
                    maximum: Some(pages),
                    ..abi::PHYSICAL_MAP
                })?;
                backend.write_memory(&mapping, 0, &bytes);
                mapping
            }
        };
        Ok(Self {
            config: Arc::new(config),
            machine_config: Arc::clone(&memory.config),
            membership: Membership::new(&machine.shared),
            context,
            guest,
            mapping,
            revision,
            identity: Arc::clone(&memory.identity),
        })
    }

    pub fn id(&self) -> CpuId {
        self.membership.id
    }

    pub(crate) fn shared_resources<A: WasmMemoryAccess<Memory = M>>(
        &self,
        backend: &A,
    ) -> MemoryResources<A::SharedMemory> {
        MemoryResources {
            backing: backend
                .shared_memory(&self.guest)
                .expect("shared backing memory"),
            mapping: match self.machine_config.memory_map() {
                MemoryMap::Physical(_) => None,
                MemoryMap::Paged(_) => Some(
                    backend
                        .shared_memory(&self.mapping)
                        .expect("shared page table"),
                ),
            },
            revision: backend
                .shared_memory(&self.revision)
                .expect("shared revision memory"),
        }
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }
    pub fn context_memory(&self) -> &M {
        &self.context
    }
    pub fn guest_memory(&self) -> &M {
        &self.guest
    }

    /// Binds the CPU, guest backing and address translation memory.
    pub fn imports<'a, B: WasmBackend<Memory = M> + 'a>(
        &'a self,
        backend: &B,
    ) -> [Import<'a, B>; 3] {
        [
            backend.bind(abi::EXECUTION_CONTEXT, &self.context),
            backend.bind(abi::GUEST, &self.guest),
            backend.bind(
                match self.machine_config.memory_map() {
                    MemoryMap::Physical(_) => abi::PHYSICAL_MAP,
                    MemoryMap::Paged(_) => abi::PAGE_TABLE,
                },
                &self.mapping,
            ),
        ]
    }
}
