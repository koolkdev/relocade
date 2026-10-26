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
//! use relocade_machine::{CpuConfig, MachineConfig};
//! use relocade_machine::{Cpu, RuntimeBackend, RuntimeState};
//!
//! async fn example<B: RuntimeBackend<State = RuntimeState>>(
//!     backend: B,
//!     config: MachineConfig,
//!     cpu: CpuConfig,
//! ) -> Result<(), Box<dyn std::error::Error>> {
//!     let mut runtime = Cpu::new(backend, config, cpu).await?;
//!     runtime.context().memory().write(0x100, &[0xb8, 42, 0, 0xeb, 0])?;
//!     let exit = runtime.run(100)?;
//!     let ax = runtime.context().cpu().ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

mod access;
mod backend;
mod callback;
mod code;
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

use crate::{CpuConfig, CpuContext, MachineConfig, Memory};
use access::CpuAccess;
pub use backend::RuntimeBackend;
pub use callback::HostCall;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use hooks::{Hook, HookResult};
pub use memory::RuntimeMemory;
use relocade_wasm_runtime::WasmCompiler;
pub use state::RuntimeState;
use std::{
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// One guest CPU and its engine resources, executable dispatch and pending calls.
pub struct Cpu<B: RuntimeBackend> {
    config: Arc<CpuConfig>,
    machine_config: Arc<MachineConfig>,
    access: CpuAccess<B>,
    dispatch: Dispatch<B>,
    host_call_incomplete: bool,
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Cpu<B> {
    /// Allocates the configured CPU and memory and links its executable dispatch.
    /// Port and MMIO reads currently return all ones; their writes are ignored.
    pub async fn new(mut backend: B, config: MachineConfig, cpu: CpuConfig) -> Result<Self> {
        let bindings = RuntimeMemory::new(&mut backend, config, cpu)?;
        let host_imports = io::unconnected_imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        Ok(Self {
            config: Arc::clone(&bindings.config),
            machine_config: Arc::clone(&bindings.machine_config),
            access: CpuAccess { backend, bindings },
            dispatch,
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
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let module = self.access.backend.compiler().compile(bytes).await?;
        self.dispatch
            .install(&mut self.access, &module, entries)
            .await?;
        for entry in entries {
            self.access.backend.state_mut().hooks.remove(&entry.address);
        }
        Ok(())
    }

    /// Borrows the stopped CPU and guest memory. Writes take effect immediately.
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext::new(&self.config, &self.machine_config, &mut self.access)
    }

    /// Shares guest memory through a handle that can outlive this runtime.
    /// Native handles can move to another thread; browser handles preserve their
    /// shared allocation during worker transport. CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory<B::SharedMemory> {
        Memory::with_backing(
            self.machine_config.as_ref().clone(),
            self.access
                .backend
                .shared_memory(&self.access.bindings.guest)
                .expect("guest backing is shared"),
        )
        .expect("guest backing matches the configured capacity")
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> impl Deref<Target = T> + '_ {
        state::Data(self.access.backend.state())
    }

    pub fn state_mut(&mut self) -> impl DerefMut<Target = T> + '_ {
        state::Data(self.access.backend.state_mut())
    }
}
