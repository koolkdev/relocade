use crate::{
    backend::{Backend, Instance},
    Result, Runtime,
};
use wasm86_machine::{ConfigurationError, Environment, Machine, MemoryAccess, MemoryError};
use wasm86_x86::{CpuState, EntryTable, ExecutionTarget};

/// Persistent CPU, address space and environment on one execution backend.
/// Dispatch tail-calls installed code or the interpreter inside Wasm. The target
/// stays fixed for the session, and its segment assumptions must remain valid
/// throughout execution. Automatic mode transitions are not provided.
pub struct Session<B: Backend, D: Environment + 'static> {
    pub(crate) instance: B::Instance<D>,
    pub(crate) runtime: Runtime<B>,
    pub(crate) entries: EntryTable,
    pub(crate) next_slot: u32,
    pub(crate) target: ExecutionTarget,
}

impl<B: Backend, D: Environment + 'static> Session<B, D> {
    pub(crate) async fn new(runtime: &Runtime<B>, machine: Machine<D>) -> Result<Self> {
        let target = machine.execution_target();
        let (cpu, imports) = machine.into_parts();
        let code = runtime.prepare(target).await?;
        let mut instance = runtime
            .code
            .backend
            .instantiate(&code.interpreter, &code.dispatcher, imports)
            .await?;
        instance.with_memory(|imports, memory| {
            imports.publish_map(memory);
        });
        instance.set_cpu(&cpu);
        let entries = EntryTable::default();
        Ok(Self {
            instance,
            runtime: runtime.clone(),
            entries,
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
            target,
        })
    }

    /// Runs until the instruction budget is consumed at a block boundary or an
    /// execution exit occurs. Whole blocks may exceed the budget; this is not an
    /// elapsed-time limit. A zero budget does not enter Wasm. The absolute target
    /// saturates at u64::MAX, which is practically unbounded.
    pub fn run(&mut self, instruction_budget: u64) -> Result<wasm86_x86::ExecutionExit> {
        use wasm86_x86::ExecutionExit;
        if instruction_budget == 0 {
            return Ok(ExecutionExit::BudgetExhausted);
        }
        let target = self
            .cpu()
            .instruction_count
            .saturating_add(instruction_budget);
        let word = self.instance.run(target)?;
        ExecutionExit::from_bits(word).ok_or(crate::Error::InvalidExit(word))
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.target
    }

    pub fn cpu(&self) -> CpuState {
        self.instance.cpu()
    }

    /// Replaces CPU state between entries after checking the active target’s
    /// segment assumptions. Other architectural state follows the x86 CPU-image contract.
    pub fn set_cpu(&mut self, cpu: CpuState) -> Result<()> {
        if !self.target.profile.is_compatible_with(&cpu.segments) {
            return Err(ConfigurationError::IncompatibleCpu.into());
        }
        self.instance.set_cpu(&cpu);
        Ok(())
    }

    /// Inspects the environment without allowing a borrow to escape the backend's scope.
    pub fn with_environment<R>(&self, inspect: impl FnOnce(&D) -> R) -> R {
        self.instance
            .inspect(|imports| inspect(imports.environment()))
    }

    pub fn with_environment_mut<R>(&mut self, update: impl FnOnce(&mut D) -> R) -> R {
        self.instance
            .with_memory(|imports, _| update(imports.environment_mut()))
    }

    /// Accesses backing and routing between entries. Changes are published before
    /// this call returns, including any replacement mapping metadata.
    pub fn with_memory<R>(&mut self, access: impl FnOnce(&mut MemoryAccess<'_>) -> R) -> R {
        self.instance
            .with_memory(|imports, memory| imports.access(memory, |_, memory| access(memory)))
    }

    /// Reads guest bytes through the configured address space and access policy.
    /// Host buffer transfers use byte-wide MMIO; CPU operands retain their widths.
    /// A paged-access error can follow a completed prefix of the transfer.
    pub fn read_memory(
        &mut self,
        address: u32,
        bytes: &mut [u8],
    ) -> std::result::Result<(), MemoryError> {
        self.instance
            .with_memory(|imports, memory| imports.read(memory, address, bytes))
    }

    /// Writes through configured routing; physical ROM/hole writes are ignored.
    /// A paged-access error can follow a completed prefix of the transfer. Initialize
    /// ROM explicitly with [`MemoryAccess::write_backing`] in [`Self::with_memory`].
    pub fn write_memory(
        &mut self,
        address: u32,
        bytes: &[u8],
    ) -> std::result::Result<(), MemoryError> {
        self.instance
            .with_memory(|imports, memory| imports.write(memory, address, bytes))
    }
}
