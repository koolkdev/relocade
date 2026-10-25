//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher and initializes
//! backend storage from [`MachineConfig`]. [`Cpu::context`] lends access
//! to the stopped CPU and mapped memory, backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] awaits host operations
//! between synchronous Wasm calls.
//!
//! A retained execution task owns the machine from its first host service until
//! execution stops. Services borrow it through ordinary async calls and keep local
//! values across waits. CPU views borrow the backend directly; their accesses need
//! no handle lookup or invocation check. Environment state is independent of the
//! backend's callback state.
//!
//! A pending service survives dropping its run waiter. Dropping the CPU ends
//! the call chain and destroys the machine without undoing completed effects.
//! A suspended service excludes embedding access to CPU and environment state;
//! independent input handles and shared guest memory remain accessible.
//!
//! ```
//! use relocade_machine::{Cpu, CpuConfig, MachineConfig};
//!
//! async fn example(config: MachineConfig, cpu: CpuConfig)
//!     -> Result<(), Box<dyn std::error::Error>>
//! {
//!     let mut runtime = Cpu::new(config, cpu).await?;
//!     runtime.context().memory().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let exit = runtime.run(100).await;
//!     let ax = runtime.context().cpu().ax();
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
#[cfg(feature = "worker")]
pub mod worker;

use crate::{CpuConfig, MachineConfig, Memory};
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
use execution::Execution;
pub use hooks::{Hook, HookResult};
use machine::Machine;
use memory::RuntimeMemory;
use relocade_wasm_runtime::{SharedMemory, WasmBackend, WasmCompiler};
use std::collections::HashMap;

type Backend = relocade_wasm_runtime::Backend;

/// Executes one guest and owns its machine, registered services and suspended calls.
pub struct Cpu<T: 'static = ()> {
    execution: Execution<T>,
    memory: Memory<SharedMemory>,
}

impl Cpu {
    /// Allocates a CPU with no environment state.
    pub async fn new(config: MachineConfig, cpu: CpuConfig) -> Result<Self> {
        Self::with_state(config, cpu, ()).await
    }
}

impl<T: 'static> Cpu<T> {
    /// Allocates the configured CPU and memory and links its executable dispatch.
    /// The environment state stays with this CPU, including during host calls.
    /// Port and MMIO reads currently return all ones; their writes are ignored.
    pub async fn with_state(config: MachineConfig, cpu: CpuConfig, state: T) -> Result<Self> {
        let mut backend = Backend::new()?;
        let memory = Memory::allocate(&mut backend, config)?;
        Self::prepare(backend, memory, cpu, state).await
    }

    /// Creates a CPU with its own registers and dispatch over existing guest memory.
    /// The supplied memory and its physical map are retained without clearing bytes.
    /// Coordinate execution when another CPU uses the same memory.
    pub async fn with_memory(memory: Memory, cpu: CpuConfig, state: T) -> Result<Self> {
        let backend = Backend::new()?;
        Self::prepare(backend, memory, cpu, state).await
    }

    async fn prepare(
        mut backend: Backend,
        memory: Memory,
        cpu: CpuConfig,
        state: T,
    ) -> Result<Self> {
        let bindings = RuntimeMemory::new(&mut backend, &memory, cpu)?;
        let host_imports = io::unconnected_imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        let machine = Machine {
            backend,
            bindings,
            state,
            dispatch,
            hooks: HashMap::new(),
        };
        Ok(Self {
            execution: Execution::new(machine),
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
    pub fn context(&mut self) -> CpuContext<'_, T> {
        self.execution.machine_mut().context()
    }

    /// Shares guest memory through a handle that can outlive this runtime.
    /// Native handles can move to another thread; browser handles preserve their
    /// shared allocation during worker transport. CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory<SharedMemory> {
        self.memory.clone()
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
