//! Shared guest resources created before choosing an execution worker.

mod cpu;
pub use cpu::CpuHandle;
pub(crate) mod execution;

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
    /// Pass a compiler service to enable background compilation, or `None` to
    /// use the bundled interpreter without opening a compiler connection.
    /// A CPU with a compiler discovers and compiles hot Real16 callees in the
    /// background. Installed code and mappings must stay stable, and candidates
    /// must have no interior hooks.
    /// A compiler connection can queue requests while the service starts.
    /// Move the returned handle to its execution worker. State is appended to
    /// [`Self::cpus`] and remains available after that worker is joined.
    pub fn create_cpu(
        &mut self,
        config: CpuConfig,
        compiler: Option<&CompilerService>,
    ) -> Result<CpuHandle> {
        let (connection, endpoint) = compiler
            .map(|compiler| crate::compiler::Connection::new(self.memory(), compiler))
            .transpose()?
            .unzip();
        let (cpu, state) = CpuHandle::new(self, config, endpoint)?;
        self.cpus.push(state);
        self.compiler_connections.extend(connection);
        Ok(cpu)
    }

    /// State in creation order, for access at stopped guest boundaries.
    /// Configure before transferring a CPU, inspect after its worker joins, or
    /// hold a [`Self::pause_cpus`] guard during a session. State handles themselves
    /// do not stop or synchronize guest execution.
    pub fn cpus(&self) -> &[CpuStateAccess] {
        &self.cpus
    }

    /// Pauses CPUs created by this machine at completed execution boundaries.
    /// The returned guard blocks guest entry and CPU host-service polling until
    /// dropped. Idle, unstarted and dropped CPUs require no acknowledgement.
    /// Concurrent pause requests serialize; cancellation releases a pending pause.
    ///
    /// Active execution stops at existing checkpoints, without a fixed latency
    /// bound for straight-line code or REP. Device activities and independent
    /// memory writers keep running. This is not an exclusive guest-memory lock,
    /// and does not include standalone CPUs created through [`crate::Cpu::new`].
    /// Request from the machine owner, outside a CPU execution or service poll.
    pub async fn pause_cpus(&self) -> impl Drop + '_ {
        self.memory
            .execution
            .pause(|| {
                for cpu in &self.cpus {
                    cpu.interrupt_handle().request();
                }
            })
            .await
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
