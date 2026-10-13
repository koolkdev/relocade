//! Persistent, single-CPU real-mode execution with the generated Wasm interpreter.
//!
//! [`Runtime`] prepares an interpreter and dispatcher for independent [`Machine`]s.
//! A [`backend::Backend`] supplies host-engine compilation, memories and callbacks;
//! the runtime owns physical routing, device access and execution policy.
//! [`Machine::step`] returns after one x86 instruction, including all REP elements,
//! or a guest exit. [`Machine::run`] repeats those entries until an instruction
//! limit, a host stop request or a guest exit. It does not preempt an instruction
//! or device callback. [`Machine::run_slice`] instead bounds instruction/REP-element
//! work, publishing partial REP progress. Neither API preempts a synchronous host
//! callback. [`Machine::install_hooks`] registers address-based host services;
//! generated Wasm calls them directly with [`HostContext`]. [`HostAction::Yield`]
//! returns published state without repeating the callback on resumption.
//! Guest exceptions are reported, not delivered through the IVT.
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
mod host;
mod imports;
mod machine;
mod memory;
mod module;
mod run;
mod slice;

pub use devices::Devices;
pub use error::{Error, Result};
pub use host::{HookReturn, Hooks, HostAction, HostContext};
pub use machine::{Machine, StepOutcome};
pub use memory::{DeviceMemory, MemoryError};
pub use module::ExecutableModule;
pub use run::{RunOutcome, RunStop};
pub use slice::{SliceOutcome, SliceStop};

use backend::Backend;
use std::sync::Arc;
use wasm86_x86::{Compiler, ExecutionProfile, PhysicalMemoryMap};

/// Host engine and prepared execution modules shared by independent machines.
pub struct Runtime<B: Backend> {
    code: Arc<RuntimeCode<B>>,
}

struct RuntimeCode<B: Backend> {
    backend: B,
    interpreter: B::Module,
    dispatcher: B::Module,
}

impl<B: Backend> Runtime<B> {
    /// Prepares the Real16 interpreter and callable dispatcher on the supplied engine.
    pub async fn new(backend: B) -> Result<Self> {
        let module = Compiler::new(ExecutionProfile::Real16)
            .with_execution_budget()
            .compile_interpreter_step()?;
        let interpreter = backend.compile(&module).await?;
        let dispatcher = backend
            .compile(
                &Compiler::new(ExecutionProfile::Real16)
                    .with_execution_budget()
                    .compile_dispatcher()?,
            )
            .await?;
        Ok(Self {
            code: Arc::new(RuntimeCode {
                backend,
                interpreter,
                dispatcher,
            }),
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

impl<B: Backend> Clone for Runtime<B> {
    fn clone(&self) -> Self {
        Self {
            code: self.code.clone(),
        }
    }
}
