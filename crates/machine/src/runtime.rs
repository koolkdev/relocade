//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher over supplied [`Memory`]
//! and initializes private CPU storage. [`Cpu::context`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] awaits host operations
//! between synchronous Wasm calls.
//!
//! The execution driver owns the machine and retains each pending invocation.
//! [`CallContext`] owns CPU access and independently retained memory/I/O resources.
//! The driver binds live CPU storage only for each future poll and completion
//! cleanup. Ordinary awaits preserve the future and its locals. Mutable environment
//! access uses synchronous [`EnvironmentAccess`] callbacks; asynchronous services
//! capture their own resources.
//!
//! A pending service survives dropping its run waiter. Dropping the CPU ends
//! the call chain and destroys the machine without undoing completed effects.
//! A suspended service excludes embedding access to CPU and environment state;
//! independent input handles and shared guest memory remain accessible.
//!
//! ```
//! use relocade_machine::{Cpu, CpuConfig, MachineConfig, Memory};
//!
//! async fn example(config: MachineConfig, cpu_config: CpuConfig)
//!     -> Result<(), Box<dyn std::error::Error>>
//! {
//!     let memory = Memory::new(config)?;
//!     memory.view().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let mut cpu = Cpu::new(memory, cpu_config, Default::default(), ()).await?;
//!     let exit = cpu.run(100).await;
//!     let ax = cpu.context().cpu.ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![deny(unsafe_code)]

mod access;
mod call;
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

use crate::{CpuConfig, IoBus, Memory};
pub use access::{CallContext, EnvironmentAccess, InvocationAccess};
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
use execution::Execution;
pub use hooks::{Hook, HookResult};
use machine::Machine;
use memory::RuntimeMemory;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use std::collections::HashMap;

type Backend = relocade_wasm_runtime::Backend<IoBus>;

/// Executes one guest and owns its machine, registered services and suspended calls.
pub struct Cpu<T: 'static = ()> {
    execution: Execution<T>,
}

impl<T: 'static> Cpu<T> {
    /// Creates private CPU state and dispatch over existing guest memory.
    /// Guest bytes are preserved. Use `()` when no environment state is needed.
    /// The environment state stays with this CPU, including during host calls.
    /// Port and MMIO accesses use the supplied execution-local device bus.
    pub async fn new(memory: Memory, cpu: CpuConfig, io: IoBus, state: T) -> Result<Self> {
        let mut backend = Backend::with_state(io.clone())?;
        let bindings = RuntimeMemory::new(&mut backend, memory, cpu)?;
        let host_imports = io::imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        let memory = bindings.guest.clone().with_io(io.clone());
        let machine = Machine {
            backend,
            bindings,
            io,
            memory,
            state,
            dispatch,
            hooks: HashMap::new(),
        };
        Ok(Self {
            execution: Execution::new(machine),
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
    /// execution belongs to `run`.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it before installing.
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let machine = self.execution.machine_mut();
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

    /// Borrows the stopped machine. The context and its views must end before
    /// running or installing modules.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it to regain access.
    pub fn context(&mut self) -> CpuContext<'_, T> {
        self.execution.machine_mut().context()
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory {
        self.execution.machine.bindings.guest.clone()
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it to regain access.
    pub fn state(&self) -> &T {
        &self.execution.machine().state
    }

    /// Borrows mutable environment state between guest calls.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it to regain access.
    pub fn state_mut(&mut self) -> &mut T {
        &mut self.execution.machine_mut().state
    }
}
