use crate::{
    backend::{Backend, EntryRequest, EntryResult, Instance},
    imports::Imports,
    DeviceMemory, Devices, Error, MemoryError, Result, Runtime,
};
use wasm86_x86::{
    CpuState, EntryTable, ExecutionExit, ExecutionProfile, PhysicalMemoryMap, Segments,
};

/// Result of one instruction attempt. CPU and completed memory/device effects
/// are published for both outcomes, including partial progress on a guest fault.
/// A host callback may yield before the instruction retires.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepOutcome {
    Completed,
    Exited(ExecutionExit),
}

/// Persistent real-mode CPU, memory and devices on one execution backend.
/// Dispatch tail-calls installed code or the interpreter inside Wasm.
pub struct Machine<B: Backend, D: Devices + 'static> {
    pub(crate) instance: B::Instance<D>,
    pub(crate) runtime: Runtime<B>,
    pub(crate) entries: EntryTable,
    pub(crate) next_slot: u32,
}

impl<B: Backend, D: Devices + 'static> Machine<B, D> {
    pub(crate) async fn new(
        runtime: &Runtime<B>,
        map: PhysicalMemoryMap,
        devices: D,
    ) -> Result<Self> {
        let required = map.required_backing_bytes();
        let mut instance = runtime
            .code
            .backend
            .instantiate(
                &runtime.code.interpreter,
                &runtime.code.dispatcher,
                Imports::new(map, devices),
            )
            .await?;
        instance.grow_guest(required)?;
        instance.with_memory(|imports, memory| imports.publish_map(memory));
        instance.set_cpu(&CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        });
        let entries = EntryTable::default();
        instance.set_entries(&entries)?;
        Ok(Self {
            instance,
            runtime: runtime.clone(),
            entries,
            next_slot: EntryTable::FIRST_DYNAMIC_SLOT,
        })
    }

    pub fn cpu(&self) -> CpuState {
        self.instance.cpu()
    }

    /// Replaces CPU state between entries after checking canonical real-mode
    /// caches. Other architectural state follows the x86 CPU-image contract.
    pub fn set_cpu(&mut self, cpu: CpuState) -> Result<()> {
        if !ExecutionProfile::Real16.is_compatible_with(&cpu.segments) {
            return Err(Error::IncompatibleCpu);
        }
        self.instance.set_cpu(&cpu);
        Ok(())
    }

    /// Abandons the saved callable continuation. The next execution resolves
    /// the current CS:EIP as a fresh visit, even if the address has not changed.
    /// Use after installing CPU state for a new process or deliberate restart.
    /// This does not undo partial instruction effects; ordinary fault repair and
    /// completion of a yielded service should preserve the continuation instead.
    pub fn restart_entry(&mut self) {
        self.instance.restart_entry();
    }

    /// Removes an address mapping. Later visits use the interpreter fallback.
    /// Installed code remains alive for a suspended continuation, if any.
    pub fn remove_entry(&mut self, address: u32) -> Result<bool> {
        let mut entries = self.entries.clone();
        if entries.remove(address).is_none() {
            return Ok(false);
        }
        self.instance.set_entries(&entries)?;
        self.entries = entries;
        Ok(true)
    }

    /// Inspects devices without allowing a borrow to escape the backend's scope.
    pub fn with_devices<R>(&self, inspect: impl FnOnce(&D) -> R) -> R {
        self.instance.inspect(|imports| inspect(&imports.devices))
    }

    pub fn with_devices_mut<R>(&mut self, update: impl FnOnce(&mut D) -> R) -> R {
        self.instance
            .with_memory(|imports, _| update(&mut imports.devices))
    }

    /// Executes one instruction, including a complete REP, or returns an exit.
    /// Host callbacks run on the way and can return a HostYield exit. Unexpected engine
    /// failures are implementation errors, not resumable guest outcomes.
    pub fn step(&mut self) -> Result<StepOutcome> {
        loop {
            let progress = self.enter(EntryRequest::step())?;
            if let Some(exit) = progress.exit {
                return Ok(StepOutcome::Exited(exit));
            }
            if progress.entry.completed != 0 {
                return Ok(StepOutcome::Completed);
            }
        }
    }

    pub(crate) fn enter(&mut self, request: EntryRequest) -> Result<Progress> {
        let entry = self.instance.enter(request)?;
        let exit = match entry.word {
            // The generated dispatcher returns zero at an observed boundary.
            0 => None,
            word => match ExecutionExit::from_bits(word) {
                Some(ExecutionExit::BudgetExhausted | ExecutionExit::InstructionLimit) => None,
                Some(exit) => Some(exit),
                None => return Err(Error::InvalidExit(word)),
            },
        };
        Ok(Progress { entry, exit })
    }

    /// Changes mappings between entries, growing zeroed backing as needed.
    pub fn set_memory_map(&mut self, map: PhysicalMemoryMap) -> Result<()> {
        self.instance.grow_guest(map.required_backing_bytes())?;
        self.with_memory(|memory| memory.set_map(map))
            .expect("backend grew backing for every mapping");
        Ok(())
    }

    /// Accesses backing and routing between entries. Changes are published before
    /// this call returns, including any replacement physical-map table.
    pub fn with_memory<R>(&mut self, access: impl FnOnce(&mut DeviceMemory<'_>) -> R) -> R {
        self.instance
            .with_memory(|imports, memory| imports.access(memory, |_, memory| access(memory)))
    }

    /// Reads physical bytes through current A20 and RAM/ROM/MMIO/hole routing.
    /// Host buffer transfers use byte-wide MMIO; CPU operands retain their widths.
    pub fn read_memory(
        &mut self,
        address: u32,
        bytes: &mut [u8],
    ) -> std::result::Result<(), MemoryError> {
        self.instance
            .with_memory(|imports, memory| imports.read(memory, address, bytes))
    }

    /// Writes through physical routing; ROM/hole writes are ignored. Initialize
    /// ROM explicitly with [`DeviceMemory::write_backing`] in [`Self::with_memory`].
    pub fn write_memory(
        &mut self,
        address: u32,
        bytes: &[u8],
    ) -> std::result::Result<(), MemoryError> {
        self.instance
            .with_memory(|imports, memory| imports.write(memory, address, bytes))
    }
}

/// Dispatch is a handoff, not proof that an instruction retired.
pub(crate) struct Progress {
    pub entry: EntryResult,
    pub exit: Option<ExecutionExit>,
}
