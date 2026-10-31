//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher over supplied [`Memory`]
//! and initializes independently retained CPU storage. [`Cpu::context`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] awaits host operations
//! between synchronous Wasm calls.
//!
//! A retained execution future owns the machine from its first host service until
//! execution stops. Services borrow it through ordinary async calls and keep local
//! values across waits. CPU state is independently retained; its value accesses need
//! no backend borrow or invocation check. Application resources belong to the
//! registered services that capture them.
//!
//! A pending service survives dropping its run waiter. Dropping the CPU ends
//! the call chain and destroys the machine without undoing completed effects.
//! A suspended service excludes borrowing the runtime context. Captured input
//! handles, application state and shared guest memory remain independently accessible.
//!
//! ```
//! use relocade_machine::{Cpu, CpuConfig, MachineConfig, Memory};
//!
//! async fn example(config: MachineConfig, cpu_config: CpuConfig)
//!     -> Result<(), Box<dyn std::error::Error>>
//! {
//!     let memory = Memory::new(config)?;
//!     memory.view().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let mut cpu = Cpu::new(memory, cpu_config, Default::default()).await?;
//!     let exit = cpu.run(100).await;
//!     let ax = cpu.context().cpu.ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

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

use crate::{CpuConfig, CpuStateAccess, IoBus, Memory};
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
pub struct Cpu {
    execution: Execution,
    cpu: CpuStateAccess,
    memory: Memory,
}

impl Cpu {
    /// Creates CPU state and dispatch over existing guest memory.
    /// Guest bytes are preserved. Host services capture their application resources.
    /// Port and MMIO accesses use the supplied execution-local device bus.
    pub async fn new(memory: Memory, cpu: CpuConfig, io: IoBus) -> Result<Self> {
        let mut backend = Backend::with_state(io.clone())?;
        let bindings = RuntimeMemory::new(&mut backend, memory.clone(), cpu)?;
        let host_imports = io::imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        let machine = Machine {
            backend,
            bindings,
            io,
            dispatch,
            hooks: HashMap::new(),
        };
        let cpu = machine.bindings.cpu.clone();
        Ok(Self {
            execution: Execution::new(machine),
            cpu,
            memory,
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
    pub fn context(&mut self) -> CpuContext<'_> {
        self.execution.machine_mut().context()
    }

    /// Retains live CPU state independently of the runner and its backend.
    /// Capture this once when registering a service, like memory and I/O handles.
    pub fn cpu_state(&self) -> CpuStateAccess {
        self.cpu.clone()
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; dispatch storage remains private.
    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }
}
