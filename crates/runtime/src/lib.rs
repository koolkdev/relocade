//! Persistent, single-CPU real-mode execution with the generated Wasm interpreter.
//!
//! [`Runtime`] compiles one interpreter for reuse across independent [`Machine`]s.
//! Machines own their CPU, physical backing and devices. [`Machine::step`] returns
//! after one x86 instruction, including all elements of REP, or a guest exit.
//! Guest exceptions are reported to the embedding; this crate does not deliver
//! exceptions or external interrupts through the IVT.
//!
//! ```
//! use wasm86_runtime::{Runtime, StepOutcome};
//! use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};
//!
//! let runtime = Runtime::new()?;
//! let map = PhysicalMemoryMap::new([
//!     (0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
//! ])?;
//! let mut machine = runtime.machine(map, ())?;
//! machine.write_memory(0x100, &[0xb8, 42, 0])?; // MOV AX, 42
//! let mut cpu = machine.cpu();
//! cpu.eip = 0x100;
//! machine.set_cpu(cpu)?;
//! assert_eq!(machine.step()?, StepOutcome::Completed);
//! assert_eq!(machine.cpu().registers.eax, 42);
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
#![forbid(unsafe_code)]

mod devices;
mod machine;
mod memory;

pub use devices::Devices;
pub use machine::{Machine, StepOutcome};
pub use memory::{DeviceMemory, MemoryError};

use wasm86_x86::{compile_interpreter_step, ExecutionProfile, PhysicalMemoryMap};
use wasmtime::{Config, Engine, Module};

/// Compiled interpreter and Wasmtime engine shared by independent machines.
pub struct Runtime {
    engine: Engine,
    interpreter: Module,
}

impl Runtime {
    /// Generates and compiles the Real16 single-instruction interpreter once.
    pub fn new() -> wasmtime::Result<Self> {
        let mut config = Config::new();
        config
            .wasm_multi_memory(true)
            .wasm_tail_call(true)
            .wasm_threads(true);
        let engine = Engine::new(&config)?;
        let module = compile_interpreter_step(ExecutionProfile::Real16)?;
        let interpreter = Module::new(&engine, &module.bytes)?;
        Ok(Self {
            engine,
            interpreter,
        })
    }

    /// Creates zeroed backing large enough for all mappings, with canonical
    /// real-mode segment caches. `()` supplies open-bus reads and ignored writes.
    pub fn machine<D: Devices + 'static>(
        &self,
        map: PhysicalMemoryMap,
        devices: D,
    ) -> wasmtime::Result<Machine<D>> {
        Machine::new(self, map, devices)
    }
}
