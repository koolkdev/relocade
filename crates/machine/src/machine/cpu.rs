use crate::memory::CpuBinding;
use crate::{
    Cpu, CpuConfig, CpuStateAccess, CpuStorage, ExecutionInterrupt, IoBus, Memory, Result,
};

/// The single execution capability for a CPU created by [`super::Machine`].
/// Moving it to a worker preserves initialized registers and guest memory.
/// It is deliberately not cloneable: state handles do not grant execution access.
#[derive(crate::worker::Transfer)]
pub struct CpuHandle {
    storage: CpuStorage,
    binding: CpuBinding,
    compiler: Option<crate::compiler::Endpoint>,
    execution: super::execution::Member,
}

impl CpuHandle {
    pub(super) fn new(
        binding: CpuBinding,
        execution: super::execution::Member,
        compiler: Option<crate::compiler::Endpoint>,
    ) -> Result<(Self, CpuStateAccess)> {
        let storage = CpuStorage::new()?;
        let backing = storage.initialize(&binding.config);
        let state = CpuStateAccess::new(&binding.config, backing);
        Ok((
            Self {
                storage,
                binding,
                compiler,
                execution,
            },
            state,
        ))
    }

    pub fn memory(&self) -> Memory {
        self.binding.memory.clone()
    }

    pub fn configuration(&self) -> &CpuConfig {
        &self.binding.config
    }

    pub fn interrupt_handle(&self) -> ExecutionInterrupt {
        self.storage.interrupt_handle()
    }

    /// Constructs this CPU's backend and device bindings on its execution owner.
    /// Existing registers, including edits made after creation, are preserved.
    pub async fn into_cpu(self, io: IoBus) -> Result<Cpu> {
        let (cpu, _compiler) = self.into_cpu_and_compiler(io).await?;
        Ok(cpu)
    }

    pub(crate) async fn into_cpu_and_compiler(
        self,
        io: IoBus,
    ) -> Result<(Cpu, Option<crate::compiler::Endpoint>)> {
        let mut cpu = self.storage.into_initialized_cpu(self.binding, io).await?;
        cpu.execution = Some(self.execution);
        Ok((cpu, self.compiler))
    }
}
