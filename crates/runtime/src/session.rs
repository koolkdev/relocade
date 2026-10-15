use crate::{
    backend::{Backend, Instance},
    Result, Runtime,
};
use wasm86_machine::{CpuContext, Environment, Machine, MemoryAccess, MemoryError, MemoryMap};
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
        let (context, imports) = machine.into_parts();
        let target = context.target();
        let required = imports.memory_map().required_backing_bytes();
        let code = runtime.prepare(target).await?;
        let mut instance = runtime
            .code
            .backend
            .instantiate(&code.interpreter, &code.dispatcher, imports)
            .await?;
        instance.grow_guest(required)?;
        instance.with_memory(|imports, memory| {
            imports.initialize_memory(memory);
        });
        instance.set_cpu(&context.cpu());
        let entries = EntryTable::default();
        instance.set_entries(&entries)?;
        Ok(Self {
            instance,
            runtime: runtime.clone(),
            entries,
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
            target,
        })
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
        CpuContext::new(self.target, cpu)?;
        self.instance.set_cpu(&cpu);
        Ok(())
    }

    /// Removes an address mapping. Later visits use the interpreter fallback.
    /// Installed code remains alive until the session is dropped.
    pub fn remove_entry(&mut self, address: u32) -> Result<bool> {
        let mut entries = self.entries.clone();
        if entries.remove(address).is_none() {
            return Ok(false);
        }
        self.instance.set_entries(&entries)?;
        self.entries = entries;
        Ok(true)
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

    /// Changes mappings between entries, growing zeroed backing as needed.
    pub fn set_memory_map(&mut self, map: impl Into<MemoryMap>) -> Result<()> {
        let map = map.into();
        if !map.supports(self.target.profile) {
            return Err(wasm86_machine::ConfigurationError::IncompatibleMemory.into());
        }
        self.instance.grow_guest(map.required_backing_bytes())?;
        self.with_memory(|memory| memory.set_map(map))
            .expect("backend grew backing for every mapping");
        Ok(())
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
