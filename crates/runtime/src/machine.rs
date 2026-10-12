use crate::{
    backend::{Backend, Instance},
    imports::Imports,
    DeviceMemory, Devices, Error, MemoryError, Result, Runtime,
};
use wasm86_x86::{CpuState, ExecutionExit, ExecutionProfile, PhysicalMemoryMap, Segments};

/// Result of one instruction attempt. CPU and completed memory/device effects
/// are published for both outcomes, including partial progress on a guest fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepOutcome {
    Completed,
    Exited(ExecutionExit),
}

/// Persistent real-mode CPU, memory and devices on one execution backend.
/// Every entry returns to the host; dispatch never recursively enters Wasm.
pub struct Machine<B: Backend, D: Devices + 'static> {
    instance: B::Instance<D>,
}

impl<B: Backend, D: Devices + 'static> Machine<B, D> {
    pub(crate) async fn new(
        runtime: &Runtime<B>,
        map: PhysicalMemoryMap,
        devices: D,
    ) -> Result<Self> {
        let required = map.required_backing_bytes();
        let mut instance = runtime
            .backend
            .instantiate(&runtime.interpreter, Imports::new(map, devices))
            .await?;
        instance.grow_guest(required)?;
        instance.with_memory(|imports, memory| imports.publish_map(memory));
        instance.set_cpu(&CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        });
        Ok(Self { instance })
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

    /// Inspects devices without allowing a borrow to escape the backend's scope.
    pub fn with_devices<R>(&self, inspect: impl FnOnce(&D) -> R) -> R {
        self.instance.inspect(|imports| inspect(&imports.devices))
    }

    pub fn with_devices_mut<R>(&mut self, update: impl FnOnce(&mut D) -> R) -> R {
        self.instance
            .with_memory(|imports, _| update(&mut imports.devices))
    }

    /// Executes one instruction, including a complete REP. Unexpected engine
    /// failures are implementation errors, not resumable guest outcomes.
    pub fn step(&mut self) -> Result<StepOutcome> {
        self.instance
            .with_memory(|imports, _| imports.dispatched = false);
        let word = self.instance.enter()?;
        self.instance.inspect(|imports| {
            if imports.dispatched {
                Ok(StepOutcome::Completed)
            } else {
                ExecutionExit::from_bits(word)
                    .map(StepOutcome::Exited)
                    .ok_or(Error::InvalidExit(word))
            }
        })
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
