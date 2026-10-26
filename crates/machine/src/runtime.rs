//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher over supplied [`Memory`]
//! and initializes private CPU storage. [`Cpu::context`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run_async`] can await host operations
//! between synchronous Wasm calls. Environment state is owned by the machine,
//! independently of the backend callback state.
//!
//! ```
//! use relocade_machine::{Cpu, CpuConfig, MachineConfig, Memory};
//!
//! async fn example(config: MachineConfig, cpu: CpuConfig)
//!     -> Result<(), Box<dyn std::error::Error>>
//! {
//!     let memory = Memory::new(config)?;
//!     memory.view().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let mut runtime = Cpu::new(memory, cpu, ()).await?;
//!     let exit = runtime.run(100)?;
//!     let ax = runtime.context().cpu.ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

mod callback;
mod code;
mod context;
mod dispatch;
mod error;
mod execution;
mod hooks;
mod io;
mod machine;
mod memory;
pub mod real_mode_interrupt;
#[cfg(feature = "worker")]
pub mod worker;

use crate::{CpuConfig, Memory};
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use hooks::{Hook, HookResult};
use machine::Machine;
use memory::RuntimeMemory;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use std::collections::HashMap;

type Backend = relocade_wasm_runtime::Backend;

/// One guest CPU and its engine resources, executable dispatch and pending calls.
pub struct Cpu<T: 'static = ()> {
    machine: Machine<T>,
    memory: Memory,
    host_call_incomplete: bool,
}

impl<T: 'static> Cpu<T> {
    /// Creates private CPU state and dispatch over existing guest memory.
    /// Guest bytes are preserved. Use `()` when no environment state is needed.
    /// The environment state stays with this CPU, including during host calls.
    /// Port and MMIO reads currently return all ones; their writes are ignored.
    pub async fn new(memory: Memory, cpu: CpuConfig, state: T) -> Result<Self> {
        let mut backend = Backend::new()?;
        let bindings = RuntimeMemory::new(&mut backend, memory.clone(), cpu)?;
        let host_imports = io::unconnected_imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        Ok(Self {
            machine: Machine {
                backend,
                bindings,
                state,
                dispatch,
                hooks: HashMap::new(),
                pending: None,
            },
            memory,
            host_call_incomplete: false,
        })
    }

    /// Links a module to this runtime and publishes its callable exports.
    /// Each export has signature `() -> ExecutionExit` and owns its entry checks, CPU
    /// publication and continuation. Addresses are linear CS.base + EIP, before
    /// paging/A20. Installing an existing address replaces its function.
    ///
    /// The caller must maintain the module's execution assumptions and invalidate
    /// or replace captured code when guest bytes or mappings change.
    /// Module start functions must not enter guest dispatch or host hooks;
    /// execution belongs to `run` and `run_async`.
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let machine = &mut self.machine;
        let compiler = machine.backend.compiler();
        let module = compiler.compile(bytes).await?;
        machine
            .dispatch
            .install(&mut machine.backend, &machine.bindings, &module, entries)
            .await?;
        for entry in entries {
            machine.hooks.remove(&entry.address);
        }
        Ok(())
    }

    /// Borrows the stopped CPU, guest memory and environment state. Writes take
    /// effect immediately. Drop this context before running the CPU.
    pub fn context(&mut self) -> CpuContext<'_, T> {
        self.machine.context()
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> &T {
        &self.machine.state
    }

    pub fn state_mut(&mut self) -> &mut T {
        &mut self.machine.state
    }
}
