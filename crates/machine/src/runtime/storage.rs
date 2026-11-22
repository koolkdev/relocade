//! CPU storage prepared before its Wasm runtime is constructed.

use super::{Cpu, ExecutionInterrupt, Result};
use crate::{memory::CpuBinding, CpuConfig, IoBus, Memory};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::{DescriptorTables, ExecutionContext};
use std::{
    mem::offset_of,
    sync::{Arc, Mutex},
};

/// Prepared storage for one CPU, including its execution-interrupt signal.
///
/// Allocate on the host and retain [`Self::interrupt_handle`] before transferring
/// this resource to an execution worker. [`Self::into_cpu`] consumes it and creates
/// the Wasm runtime on that worker. Neither operation starts guest execution.
#[derive(crate::worker::Transfer)]
pub struct CpuStorage {
    pub(super) context: SharedMemory,
    pub(super) descriptors: Option<Arc<Mutex<DescriptorTables>>>,
}

impl CpuStorage {
    pub fn new() -> Result<Self> {
        let storage = SharedMemory::new(1, 1)?;
        // Initialize all execution controls before publishing notification access.
        storage.write(0, &ExecutionContext::default().to_bytes());
        Ok(Self {
            context: storage,
            descriptors: None,
        })
    }

    /// Installs this flat CPU's host-managed descriptor tables before worker transfer.
    /// The initial CPU image separately specifies the already-loaded caches.
    pub fn with_descriptor_tables(mut self, tables: DescriptorTables) -> Self {
        self.descriptors = Some(Arc::new(Mutex::new(tables)));
        self
    }

    /// Notification access remains valid while CPU construction is pending.
    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        ExecutionInterrupt::new(self.context.clone())
    }

    /// Constructs the Wasm runtime over this storage and existing guest RAM.
    /// CPU initialization preserves any concurrent execution-interrupt request.
    pub async fn into_cpu(self, memory: Memory, config: CpuConfig, io: IoBus) -> Result<Cpu> {
        self.attach(memory, config, Some(io)).await
    }

    /// Constructs a flat CPU over this storage on the receiving worker.
    pub async fn into_cpu_without_io(self, memory: Memory, config: CpuConfig) -> Result<Cpu> {
        self.attach(memory, config, None).await
    }

    async fn attach(self, memory: Memory, config: CpuConfig, io: Option<IoBus>) -> Result<Cpu> {
        let binding = memory.attach_cpu(config)?;
        self.initialize(&binding.config);
        self.into_initialized_cpu(binding, io).await
    }

    pub(crate) async fn into_initialized_cpu(
        self,
        binding: CpuBinding,
        io: Option<IoBus>,
    ) -> Result<Cpu> {
        Cpu::with_storage(binding, io, self).await
    }

    pub(crate) fn initialize(&self, config: &CpuConfig) -> SharedMemory {
        let initial = ExecutionContext {
            cpu: *config.cpu(),
            cpu_model: config.execution_target().cpu_model,
            ..ExecutionContext::default()
        }
        .to_bytes();
        // Encoding remains owned by x86. The live notification byte is outside
        // this initialization range and may already be written by another thread.
        let end = offset_of!(ExecutionContext, interrupt_requested);
        self.context.write(0, &initial[..end]);
        self.context.clone()
    }
}
