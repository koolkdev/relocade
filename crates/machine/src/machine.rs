//! Shared guest resources created before choosing an execution worker.

mod cpu;
pub use cpu::CpuHandle;

use crate::{CompilerService, CpuConfig, CpuStateAccess, MachineConfig, Memory, Result};
use futures_util::future::join_all;

/// Owns guest memory, every CPU's state and their compiler connection lifetimes.
/// CPU handles grant execution access once; the machine retains state after the
/// worker finishes. Backends and devices are constructed on execution workers.
pub struct Machine {
    memory: Memory,
    cpus: Vec<CpuStateAccess>,
    compiler_connections: Vec<crate::compiler::Connection>,
}

impl Machine {
    /// Allocates shared memory before creating CPUs and connecting them to a compiler.
    /// Await [`Self::shutdown`] after execution owners have finished.
    pub fn new(config: MachineConfig) -> Result<Self> {
        let memory = Memory::new(config)?;
        Ok(Self {
            memory,
            cpus: Vec::new(),
            compiler_connections: Vec::new(),
        })
    }

    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    /// Creates and initializes one CPU without starting guest execution.
    /// Its direct connection can queue requests while the compiler service starts.
    /// Move the returned handle to its execution worker. State is appended to
    /// [`Self::cpus`] and remains available after that worker is joined.
    pub fn create_cpu(
        &mut self,
        config: CpuConfig,
        compiler: &CompilerService,
    ) -> Result<CpuHandle> {
        let (connection, endpoint) = crate::compiler::Connection::new(self.memory(), compiler)?;
        let (cpu, state) = CpuHandle::new(self.memory(), config, endpoint)?;
        self.cpus.push(state);
        self.compiler_connections.push(connection);
        Ok(cpu)
    }

    /// State in creation order, for access at stopped guest boundaries.
    /// Configure before transferring a CPU, or inspect after its worker joins.
    /// Access during a session requires an explicit stopped handshake with its
    /// execution owner; these handles do not stop or synchronize guest execution.
    pub fn cpus(&self) -> &[CpuStateAccess] {
        &self.cpus
    }

    /// Closes this machine's compiler connections and drains their accepted work.
    /// Other machines can continue using the compiler service.
    /// Other execution owners must already be stopped or be joined independently.
    pub async fn shutdown(self) -> Result<()> {
        join_all(
            self.compiler_connections
                .into_iter()
                .map(|connection| connection.shutdown()),
        )
        .await
        .into_iter()
        .collect()
    }
}
