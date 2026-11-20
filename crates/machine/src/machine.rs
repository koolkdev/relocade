//! Shared guest resources created before choosing an execution worker.

mod cpu;
pub use cpu::CpuHandle;
pub(crate) mod execution;

use crate::{CompilerService, CpuConfig, CpuStateAccess, MachineConfig, Memory, Result};
use std::sync::Arc;

/// Owns one PC's guest memory, CPU state and optional compiler connection.
/// The CPU handle grants execution access once. The machine retains its CPU
/// after the worker finishes.
pub struct Machine {
    memory: Memory,
    cpu: Option<MachineCpu>,
    execution: Arc<execution::Execution>,
}

struct MachineCpu {
    state: CpuStateAccess,
    // The machine owns its CPU even after its execution handle has been dropped.
    _binding: crate::memory::CpuBinding,
    compiler: Option<crate::compiler::Connection>,
}

impl Machine {
    /// Allocates shared memory before creating the PC's CPU.
    /// Await [`Self::shutdown`] after execution owners have finished.
    pub fn new(config: MachineConfig) -> Result<Self> {
        if config.physical_map().is_none() {
            return Err(crate::ConfigurationError::UnsupportedMemoryBackend.into());
        }
        let memory = Memory::new(config)?;
        Ok(Self {
            memory,
            cpu: None,
            execution: Arc::default(),
        })
    }

    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    /// Creates and initializes this PC's sole CPU without starting guest execution.
    /// A second attachment to the same memory returns
    /// [`crate::ConfigurationError::CpuAlreadyAttached`].
    /// Pass a compiler service to enable background compilation, or `None` to
    /// use the bundled interpreter without opening a compiler connection.
    /// A CPU with a compiler discovers and compiles hot Real16 callees in the
    /// background. It checks the captured bytes before installing a result.
    /// Retire modules affected by a write before resuming the CPU. Discovered
    /// functions must have no interior hooks.
    /// A compiler connection can queue requests while the service starts.
    /// Move the returned handle to its execution worker. Its state remains
    /// available through [`Self::cpus`] after that worker is joined.
    pub fn create_cpu(
        &mut self,
        config: CpuConfig,
        compiler: Option<&CompilerService>,
    ) -> Result<CpuHandle> {
        let binding = self.memory.attach_cpu(config)?;
        let (connection, endpoint) = compiler
            .map(|compiler| crate::compiler::Connection::new(self.memory(), compiler))
            .transpose()?
            .unzip();
        let (cpu, state) = CpuHandle::new(binding.clone(), self.execution.member(), endpoint)?;
        self.cpu = Some(MachineCpu {
            state,
            _binding: binding,
            compiler: connection,
        });
        Ok(cpu)
    }

    /// The CPU state, if created, for access at stopped guest boundaries.
    /// Configure before transferring a CPU, inspect after its worker joins, or
    /// hold a [`Self::pause_cpus`] guard during a session. State handles themselves
    /// do not stop or synchronize guest execution.
    pub fn cpus(&self) -> &[CpuStateAccess] {
        self.cpu
            .as_ref()
            .map_or(&[], |cpu| std::slice::from_ref(&cpu.state))
    }

    /// Pauses the CPU created by this machine at a completed execution boundary.
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
        self.execution
            .pause(|| {
                if let Some(cpu) = &self.cpu {
                    cpu.state.interrupt_handle().request();
                }
            })
            .await
    }

    /// Closes this machine's compiler connection and drains its accepted work.
    /// Other machines can continue using the compiler service.
    /// Other execution owners must already be stopped or be joined independently.
    pub async fn shutdown(self) -> Result<()> {
        if let Some(connection) = self.cpu.and_then(|cpu| cpu.compiler) {
            connection.shutdown().await?;
        }
        Ok(())
    }
}
