//! CPU storage prepared before its Wasm runtime is constructed.

use super::{Cpu, ExecutionInterrupt, Result};
use crate::{CpuConfig, IoBus, Memory};
use relocade_wasm_runtime::{SharedMemory, WasmSharedMemory};
use relocade_x86::ExecutionContext;
use std::mem::offset_of;

/// Prepared storage for one CPU, including its execution-interrupt signal.
///
/// Allocate on the host and retain [`Self::interrupt_handle`] before transferring
/// this resource to an execution worker. [`Self::into_cpu`] consumes it and creates
/// the Wasm runtime on that worker. Neither operation starts guest execution.
#[derive(crate::worker::Transfer)]
pub struct CpuStorage(SharedMemory);

impl CpuStorage {
    pub fn new() -> Result<Self> {
        let storage = SharedMemory::new(1, 1)?;
        // Initialize all execution controls before publishing notification access.
        storage.write(0, &ExecutionContext::default().to_bytes());
        Ok(Self(storage))
    }

    /// Notification access remains valid while CPU construction is pending.
    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        ExecutionInterrupt::new(self.0.clone())
    }

    /// Constructs the Wasm runtime over this storage and existing guest RAM.
    /// CPU initialization preserves any concurrent execution-interrupt request.
    pub async fn into_cpu(self, memory: Memory, config: CpuConfig, io: IoBus) -> Result<Cpu> {
        self.initialize(&config);
        self.into_initialized_cpu(memory, config, io).await
    }

    pub(crate) async fn into_initialized_cpu(
        self,
        memory: Memory,
        config: CpuConfig,
        io: IoBus,
    ) -> Result<Cpu> {
        Cpu::with_storage(memory, config, io, self.0).await
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
        self.0.write(0, &initial[..end]);
        self.0.clone()
    }
}
