//! Persistent, single-CPU real-mode execution with the generated Wasm interpreter.
//!
//! [`Runtime`] prepares one interpreter for reuse across independent [`Machine`]s.
//! A [`backend::Backend`] supplies host-engine compilation, memories and callbacks;
//! the runtime owns physical routing, device access and execution policy.
//! [`Machine::step`] returns after one x86 instruction, including all REP elements,
//! or a guest exit. [`Machine::run`] repeats those entries until an instruction
//! limit, a host stop request or a guest exit. It does not preempt an instruction
//! or device callback. Guest exceptions are reported, not delivered through the IVT.
//!
//! Preparation is asynchronous to accommodate browser module compilation. Generating
//! interpreter bytes, guest entry and device callbacks remain synchronous. A browser
//! embedding should run on a worker and yield to its event loop between runs.
//!
//! ```
//! use wasm86_runtime::{backend::Backend, Runtime, StepOutcome};
//! use wasm86_x86::{PhysicalMapping, PhysicalMemoryMap};
//!
//! async fn example(backend: impl Backend) -> Result<(), Box<dyn std::error::Error>> {
//!     let runtime = Runtime::new(backend).await?;
//!     let map = PhysicalMemoryMap::new([
//!         (0..=0xffff, PhysicalMapping::Ram { backing_offset: 0 }),
//!     ])?;
//!     let mut machine = runtime.machine(map, ()).await?;
//!     machine.write_memory(0x100, &[0xb8, 42, 0])?; // MOV AX, 42
//!     let mut cpu = machine.cpu();
//!     cpu.eip = 0x100;
//!     machine.set_cpu(cpu)?;
//!     assert_eq!(machine.step()?, StepOutcome::Completed);
//!     assert_eq!(machine.cpu().registers.eax, 42);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

pub mod backend;
mod devices;
mod error;
mod imports;
mod machine;
mod memory;
mod run;

pub use devices::Devices;
pub use error::{Error, Result};
pub use machine::{Machine, StepOutcome};
pub use memory::{DeviceMemory, MemoryError};
pub use run::{RunOutcome, RunStop};

use backend::Backend;
use wasm86_x86::{compile_interpreter_step, ExecutionProfile, PhysicalMemoryMap};

/// Host engine and prepared interpreter shared by independent machines.
pub struct Runtime<B: Backend> {
    backend: B,
    interpreter: B::Module,
}

impl<B: Backend> Runtime<B> {
    /// Generates and prepares the Real16 interpreter once on the supplied engine.
    pub async fn new(backend: B) -> Result<Self> {
        let module = compile_interpreter_step(ExecutionProfile::Real16)?;
        let interpreter = backend.compile(&module).await?;
        Ok(Self {
            backend,
            interpreter,
        })
    }

    /// Creates zeroed backing for all mappings and canonical real-mode CPU state.
    /// `()` supplies open-bus reads and ignored writes.
    pub async fn machine<D: Devices + 'static>(
        &self,
        map: PhysicalMemoryMap,
        devices: D,
    ) -> Result<Machine<B, D>> {
        Machine::new(self, map, devices).await
    }
}

impl<B: Backend + Clone> Clone for Runtime<B>
where
    B::Module: Clone,
{
    fn clone(&self) -> Self {
        Self {
            backend: self.backend.clone(),
            interpreter: self.interpreter.clone(),
        }
    }
}
