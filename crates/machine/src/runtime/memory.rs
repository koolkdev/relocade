//! Wasm memory resources and scoped access for runtime execution.

use super::{HostCall, Result, RuntimeState};
use crate::{CpuConfig, CpuContext, CpuView, MachineConfig};
use relocade_compiler::abi::Memory;
use relocade_wasm_runtime::{Import, WasmBackend, WasmContext, WasmMemoryAccess, WasmSharedMemory};
use relocade_x86::{abi, ExecutionContext};
use std::{mem::offset_of, sync::Arc};

/// The runtime's initialized Wasm memories. This owner allocates backend memory
/// from the machine configuration and supplies module bindings. The CPU's
/// execution owner lends host access during runs.
#[derive(Clone)]
pub struct RuntimeMemory<M, S> {
    pub(crate) config: Arc<CpuConfig>,
    pub(crate) machine_config: Arc<MachineConfig>,
    pub(crate) context: M,
    pub(crate) guest: M,
    pub(crate) mapping: M,
    pub(crate) shared_guest: S,
}

impl<M: Clone + 'static, S: WasmSharedMemory> RuntimeMemory<M, S> {
    pub fn new<B: WasmBackend<Memory = M, SharedMemory = S>>(
        backend: &mut B,
        machine_config: MachineConfig,
        config: CpuConfig,
    ) -> Result<Self> {
        machine_config.validate_for(config.execution_target().profile)?;
        let context = backend.create_memory(Memory {
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
        let guest = backend.create_memory(Memory {
            minimum: guest_pages,
            ..abi::GUEST
        })?;
        let mapping_bytes = machine_config.memory_map().to_bytes();
        let mapping_pages = mapping_bytes.len().div_ceil(65536).max(1) as u32;
        let mapping = backend.create_memory(Memory {
            minimum: mapping_pages,
            maximum: Some(mapping_pages),
            ..abi::PHYSICAL_MAP
        })?;
        backend.memory(&mapping).write(0, &mapping_bytes);
        let shared_guest = backend
            .shared_memory(&guest)
            .expect("guest backing is shared");
        Ok(Self {
            config: Arc::new(config),
            machine_config: Arc::new(machine_config),
            context,
            guest,
            mapping,
            shared_guest,
        })
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

    pub(crate) fn cpu<'a>(
        &'a self,
        access: &'a mut impl WasmMemoryAccess<Memory = M>,
    ) -> CpuView<'a> {
        CpuView::new(&self.config, access.memory(&self.context))
    }

    pub(crate) fn context<'a>(
        &'a self,
        access: &'a mut impl WasmMemoryAccess<Memory = M>,
    ) -> CpuContext<'a> {
        CpuContext::new(
            &self.config,
            &self.machine_config,
            access.memory(&self.context),
            self.shared_guest.view(),
        )
    }

    pub(crate) fn host_call<'a, T>(
        &'a self,
        access: &'a mut impl WasmContext<State = RuntimeState<T>, Memory = M>,
    ) -> HostCall<'a, T> {
        let (context, state) = access.memory_and_state(&self.context);
        HostCall::new(
            CpuContext::new(
                &self.config,
                &self.machine_config,
                context,
                self.shared_guest.view(),
            ),
            state,
        )
    }

    pub(crate) fn set_instruction_target(
        &self,
        access: &mut impl WasmMemoryAccess<Memory = M>,
        target: u64,
    ) {
        access.memory(&self.context).store(
            offset_of!(ExecutionContext, instruction_target) as u32,
            target,
        );
    }
}
