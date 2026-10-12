use crate::{devices, memory::MemoryBus, DeviceMemory, Devices, MemoryError, Runtime};
use wasm86_x86::{CpuState, ExecutionExit, ExecutionProfile, PhysicalMemoryMap, Segments};
use wasmtime::{Caller, Linker, Memory, MemoryType, Store, TypedFunc};

/// Result of one instruction attempt. CPU and completed memory/device effects
/// are published for both outcomes, including partial progress on a guest fault.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepOutcome {
    Completed,
    Exited(ExecutionExit),
}

pub(crate) struct State<D> {
    pub(crate) devices: D,
    pub(crate) map: PhysicalMemoryMap,
    pub(crate) map_changed: bool,
    dispatched: bool,
}

/// Persistent real-mode CPU, memory and devices in one Wasmtime store.
/// Every entry returns to the host; dispatch never recursively enters Wasm.
pub struct Machine<D> {
    store: Store<State<D>>,
    cpu: Memory,
    memory: MemoryBus,
    step: TypedFunc<(), i64>,
}

impl<D: Devices + 'static> Machine<D> {
    pub(crate) fn new(
        runtime: &Runtime,
        map: PhysicalMemoryMap,
        devices: D,
    ) -> wasmtime::Result<Self> {
        let mut store = Store::new(
            &runtime.engine,
            State {
                devices,
                map,
                map_changed: false,
                dispatched: false,
            },
        );
        let cpu = Memory::new(&mut store, MemoryType::new(1, Some(1)))?;
        let memory = MemoryBus::new(&mut store)?;
        let mut linker = Linker::new(&runtime.engine);
        linker.define(&store, "wasm86", "cpuState", cpu)?;
        memory.define_imports(&mut linker, &store)?;
        linker.func_wrap(
            "wasm86",
            "dispatch",
            |mut caller: Caller<'_, State<D>>, _eip: i32| {
                caller.data_mut().dispatched = true;
                0_i64
            },
        )?;
        devices::register(&mut linker, memory)?;
        let instance = linker.instantiate(&mut store, &runtime.interpreter)?;
        let step = instance.get_typed_func::<(), i64>(&mut store, "step")?;
        let mut machine = Self {
            store,
            cpu,
            memory,
            step,
        };
        machine.set_cpu(CpuState {
            segments: Segments::real_mode(),
            ..CpuState::default()
        })?;
        Ok(machine)
    }

    pub fn cpu(&self) -> CpuState {
        CpuState::from_bytes(
            self.cpu.data(&self.store)[..CpuState::BYTE_LEN]
                .try_into()
                .unwrap(),
        )
    }

    /// Replaces CPU state between entries after checking canonical real-mode
    /// caches. Other architectural state follows the x86 CPU-image contract.
    pub fn set_cpu(&mut self, cpu: CpuState) -> wasmtime::Result<()> {
        if !ExecutionProfile::Real16.is_compatible_with(&cpu.segments) {
            return Err(wasmtime::Error::msg(
                "CPU segment caches are incompatible with Real16",
            ));
        }
        self.cpu.write(&mut self.store, 0, &cpu.to_bytes())?;
        Ok(())
    }

    pub fn devices(&self) -> &D {
        &self.store.data().devices
    }
    pub fn devices_mut(&mut self) -> &mut D {
        &mut self.store.data_mut().devices
    }

    /// Executes one instruction, including a complete REP. This is an instruction
    /// boundary, not a wall-clock bound. Unexpected Wasm/host failures are returned
    /// as implementation errors; they are not resumable guest execution outcomes.
    pub fn step(&mut self) -> wasmtime::Result<StepOutcome> {
        self.store.data_mut().dispatched = false;
        let word = self.step.call(&mut self.store, ())?;
        if self.store.data().dispatched {
            Ok(StepOutcome::Completed)
        } else {
            let exit = ExecutionExit::from_bits(word as u64).ok_or_else(|| {
                wasmtime::Error::msg(format!("invalid guest exit word {word:#018x}"))
            })?;
            Ok(StepOutcome::Exited(exit))
        }
    }

    /// Changes mappings between entries, growing zeroed backing as needed.
    pub fn set_memory_map(&mut self, map: PhysicalMemoryMap) -> wasmtime::Result<()> {
        self.memory.set_map(&mut self.store, map)
    }

    /// Gives the embedding access to backing and routing between entries. A changed
    /// map is installed before execution resumes, just as during device callbacks.
    pub fn with_memory<R>(&mut self, access: impl FnOnce(&mut DeviceMemory<'_>) -> R) -> R {
        self.memory
            .access(&mut self.store, |_, memory| access(memory))
    }

    /// Reads physical bytes through current A20 and RAM/ROM/MMIO/hole routing.
    /// Host buffer transfers issue byte-wide MMIO accesses; CPU operands retain
    /// the widths specified by the x86 physical-memory contract.
    pub fn read_memory(&mut self, address: u32, bytes: &mut [u8]) -> Result<(), MemoryError> {
        self.memory.read(&mut self.store, address, bytes)
    }

    /// Writes physical bytes through current routing. ROM/hole writes are ignored;
    /// use [`Self::with_memory`] for explicit initialization of ROM backing.
    pub fn write_memory(&mut self, address: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        self.memory.write(&mut self.store, address, bytes)
    }
}
