//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher and initializes
//! backend storage from [`MachineConfig`]. [`Cpu::context`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run_async`] can await host operations
//! between synchronous Wasm calls.
//!
//! ```
//! use relocade_machine::{Cpu, CpuConfig, MachineConfig};
//!
//! async fn example(config: MachineConfig, cpu: CpuConfig)
//!     -> Result<(), Box<dyn std::error::Error>>
//! {
//!     let mut runtime = Cpu::new(config, cpu).await?;
//!     runtime.context().memory().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let exit = runtime.run(100)?;
//!     let ax = runtime.context().cpu().ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

mod code;
mod context;
mod deferred;
mod dispatch;
mod error;
mod execution;
mod hooks;
mod io;
mod memory;
pub mod real_mode_interrupt;
mod state;
#[cfg(feature = "worker")]
pub mod worker;

use crate::{CpuConfig, MachineConfig, Memory};
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use hooks::{Hook, HookResult};
use memory::RuntimeMemory;
use relocade_wasm_runtime::{SharedMemory, WasmBackend, WasmCompiler, WasmContext};
use state::RuntimeState;
use std::{
    collections::HashMap,
    ops::{Deref, DerefMut},
};

type Backend<T> = relocade_wasm_runtime::Backend<RuntimeState<T>>;

/// One guest CPU and its engine resources, executable dispatch and pending calls.
pub struct Cpu<T: 'static = ()> {
    backend: Backend<T>,
    bindings: RuntimeMemory,
    dispatch: Dispatch<T>,
    hooks: HashMap<u32, Box<hooks::Handler<T>>>,
    host_call_incomplete: bool,
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
        let mut backend = Backend::with_state(RuntimeState::new(state))?;
        let bindings = RuntimeMemory::new(&mut backend, config, cpu)?;
        let host_imports = io::unconnected_imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        Ok(Self {
            backend,
            bindings,
            dispatch,
            hooks: HashMap::new(),
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
        let module = self.backend.compiler().compile(bytes).await?;
        self.dispatch
            .install(&mut self.backend, &self.bindings, &module, entries)
            .await?;
        for entry in entries {
            self.hooks.remove(&entry.address);
        }
        Ok(())
    }

    /// Borrows the stopped CPU, guest memory and environment state. Writes take
    /// effect immediately. Drop this context before running the CPU.
    pub fn context(&mut self) -> CpuContext<'_, T> {
        self.bindings.context(&mut self.backend)
    }

    /// Shares guest memory through a handle that can outlive this runtime.
    /// Native handles can move to another thread; browser handles preserve their
    /// shared allocation during worker transport. CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory<SharedMemory> {
        Memory::with_backing(
            self.bindings.machine_config.clone(),
            self.bindings.shared_guest.clone(),
        )
        .expect("guest backing matches the configured capacity")
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> impl Deref<Target = T> + '_ {
        state::Data(self.backend.state())
    }

    pub fn state_mut(&mut self) -> impl DerefMut<Target = T> + '_ {
        state::Data(self.backend.state_mut())
    }
}
