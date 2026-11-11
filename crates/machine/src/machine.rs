//! Shared guest resources created before choosing an execution worker.

mod cpu;
pub use cpu::CpuHandle;

use crate::{worker::PoolHandle, CpuConfig, CpuStateAccess, MachineConfig, Memory, Result};

/// Owns guest memory, its compilation coordinator and every CPU's state.
/// CPU handles grant execution access once; the machine retains state after the
/// worker finishes. Backends and devices are constructed on execution workers.
pub struct Machine {
    memory: Memory,
    cpus: Vec<CpuStateAccess>,
    compiler: crate::compiler::Service,
}

impl Machine {
    /// Allocates shared memory and starts its compilation coordinator.
    /// Share `compiler_jobs` between machines and drive its pool through shutdown.
    /// CPU connections and compilation requests can queue while the worker starts.
    /// Await [`Self::shutdown`] after execution owners have finished.
    pub fn new(config: MachineConfig, compiler_jobs: PoolHandle) -> Result<Self> {
        let memory = Memory::new(config)?;
        let compiler = crate::compiler::Service::new(memory.clone(), compiler_jobs)?;
        Ok(Self {
            memory,
            cpus: Vec::new(),
            compiler,
        })
    }

    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    /// Creates and initializes one CPU without starting guest execution.
    /// Move the returned handle to its execution worker. State is appended to
    /// [`Self::cpus`] and remains available after that worker is joined.
    pub fn create_cpu(&mut self, config: CpuConfig) -> Result<CpuHandle> {
        let compiler = self.compiler.connect()?;
        let (cpu, state) = CpuHandle::new(self.memory(), config, compiler)?;
        self.cpus.push(state);
        Ok(cpu)
    }

    /// State in creation order, for access at stopped guest boundaries.
    /// Configure before transferring a CPU, or inspect after its worker joins.
    /// Access during a session requires an explicit stopped handshake with its
    /// execution owner; these handles do not stop or synchronize guest execution.
    pub fn cpus(&self) -> &[CpuStateAccess] {
        &self.cpus
    }

    /// Closes compiler admission and joins accepted work before releasing its worker.
    /// Other execution owners must already be stopped or be joined independently.
    pub async fn shutdown(self) -> Result<()> {
        self.compiler.shutdown().await
    }
}
