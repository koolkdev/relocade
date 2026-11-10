use crate::{
    Cpu, CpuConfig, CpuStateAccess, CpuStorage, ExecutionInterrupt, IoBus, Memory, Result,
};

/// The single execution capability for a CPU created by [`super::Machine`].
/// Moving it to a worker preserves initialized registers and guest memory.
/// It is deliberately not cloneable: state handles do not grant execution access.
#[derive(crate::worker::Transfer)]
pub struct CpuHandle {
    storage: CpuStorage,
    memory: Memory,
    config: CpuConfig,
}

impl CpuHandle {
    pub(super) fn new(memory: Memory, config: CpuConfig) -> Result<(Self, CpuStateAccess)> {
        memory
            .configuration()
            .validate_for(config.execution_target().profile)?;
        let storage = CpuStorage::new()?;
        let backing = storage.initialize(&config);
        let state = CpuStateAccess::new(&config, backing);
        Ok((
            Self {
                storage,
                memory,
                config,
            },
            state,
        ))
    }

    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.config
    }

    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        self.storage.interrupt_handle()
    }

    /// Constructs this CPU's backend and device bindings on its execution owner.
    /// Existing registers, including edits made after creation, are preserved.
    pub async fn into_cpu(self, io: IoBus) -> Result<Cpu> {
        self.storage
            .into_initialized_cpu(self.memory, self.config, io)
            .await
    }
}
