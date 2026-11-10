//! Shared guest resources created before choosing an execution worker.

mod cpu;
pub use cpu::CpuHandle;

use crate::{CpuConfig, CpuStateAccess, MachineConfig, Memory, Result};

/// Owns guest memory and the state of every CPU created for it.
/// CPU handles grant execution access once; the machine retains state after the
/// worker finishes. Backends and devices are constructed on execution workers.
pub struct Machine {
    memory: Memory,
    cpus: Vec<CpuStateAccess>,
}

impl Machine {
    pub fn new(config: MachineConfig) -> Result<Self> {
        Ok(Self {
            memory: Memory::new(config)?,
            cpus: Vec::new(),
        })
    }

    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    /// Creates and initializes one CPU without starting guest execution.
    /// Move the returned handle to its execution worker. State is appended to
    /// [`Self::cpus`] and remains available after that worker is joined.
    pub fn create_cpu(&mut self, config: CpuConfig) -> Result<CpuHandle> {
        let (cpu, state) = CpuHandle::new(self.memory(), config)?;
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
}
