//! CPU and guest-memory bindings for Wasm modules and published callbacks.

use super::Result;
use crate::{CpuConfig, CpuContext, CpuView, MachineConfig, MemoryView};
use relocade_compiler::abi::Memory;
use relocade_wasm_runtime::{Import, MemoryBytes, WasmBackend, WasmMemoryAccess};
use relocade_x86::{abi, ExecutionContext};
use std::{mem::offset_of, sync::Arc};

/// Initialized CPU, guest backing and physical-map bindings for one backend.
/// Supplies module imports and access to published CPU state. Architectural
/// access policy belongs to the CPU and memory views.
#[derive(Clone)]
pub struct CpuBindings<M> {
    pub(crate) config: Arc<CpuConfig>,
    pub(crate) machine_config: Arc<MachineConfig>,
    pub(crate) context: M,
    pub(crate) guest: M,
    pub(crate) mapping: M,
}

impl<M: Clone + Send + Sync + 'static> CpuBindings<M> {
    pub fn new<B: WasmBackend<Memory = M>>(
        backend: &mut B,
        machine_config: MachineConfig,
        config: CpuConfig,
    ) -> Result<Self> {
        machine_config.validate_for(config.execution_target().profile)?;
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
        backend.write_memory(&mapping, 0, &mapping_bytes);
        Ok(Self {
            config: Arc::new(config),
            machine_config: Arc::new(machine_config),
            context,
            guest,
            mapping,
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

    pub(crate) fn cpu<'a, A: WasmMemoryAccess<Memory = M>>(
        &'a self,
        backend: &'a mut A,
    ) -> CpuView<'a> {
        CpuView::new(&self.config, backend.memory_bytes(&self.context))
    }

    pub(crate) fn set_instruction_target<A: WasmMemoryAccess<Memory = M>>(
        &self,
        backend: &mut A,
        target: u64,
    ) {
        backend.write_memory(
            &self.context,
            offset_of!(ExecutionContext, instruction_target) as u32,
            &target.to_le_bytes(),
        );
    }

    /// Lends published CPU state and mapped guest memory to a host callback.
    /// The caller must publish CPU state before access and observe edits afterward.
    /// Port/MMIO imports do not provide this boundary. Guest execution must remain
    /// stopped until the views are released. Earlier edits survive handler errors.
    pub fn with_context<A: WasmMemoryAccess<Memory = M>, R>(
        &self,
        backend: &mut A,
        handler: impl FnOnce(&mut CpuContext<'_>) -> R,
    ) -> R {
        let guest = backend
            .shared_memory(&self.guest)
            .expect("guest backing is shared");
        let memory = MemoryView::new(&self.machine_config, MemoryBytes::shared(&guest));
        handler(&mut CpuContext::new(self.cpu(backend), memory))
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
