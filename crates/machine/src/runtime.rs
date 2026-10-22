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
//! use relocade_machine::{Cpu, RuntimeBackend};
//!
//! async fn example<B: RuntimeBackend>(
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

mod backend;
mod bindings;
mod code;
mod dispatch;
mod error;
mod execution;
mod hooks;
mod host;
mod io;
pub mod real_mode_interrupt;
#[cfg(feature = "worker")]
pub mod worker;

use crate::{CpuConfig, CpuContext, MachineConfig, Memory};
pub use backend::RuntimeBackend;
pub use bindings::CpuBindings;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use hooks::Hook;
pub use host::{HostAction, HostOperation};
pub use io::IoBindings;
use relocade_wasm_runtime::WasmCompiler;

/// One guest CPU and its engine resources, executable dispatch and pending calls.
pub struct Cpu<B: RuntimeBackend> {
    backend: B,
    bindings: CpuBindings<B::Memory>,
    memory: Memory<B::SharedMemory>,
    dispatch: Dispatch<B>,
    execution: execution::Execution,
}

impl<B: RuntimeBackend> Cpu<B> {
    /// Allocates a CPU and memory with unconnected port and MMIO callbacks.
    /// Reads return all ones; writes are ignored.
    pub async fn new(backend: B, config: MachineConfig, cpu: CpuConfig) -> Result<Self> {
        Self::with_io(backend, config, cpu, IoBindings::default()).await
    }

    /// Allocates a CPU and memory and binds the supplied inline device callbacks.
    pub async fn with_io(
        mut backend: B,
        config: MachineConfig,
        cpu: CpuConfig,
        io: IoBindings<B>,
    ) -> Result<Self> {
        let bindings = CpuBindings::new(&mut backend, config, cpu)?;
        let memory = Memory::with_backing(
            bindings.machine_config.as_ref().clone(),
            backend
                .shared_memory(&bindings.guest)
                .expect("guest backing is shared"),
        )?;
        let imports = io.bind(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, imports).await?;
        Ok(Self {
            backend,
            bindings,
            memory,
            dispatch,
            execution: execution::Execution::Ready,
        })
    }

    /// Links a module to this runtime and publishes its callable exports.
    /// Each export has signature `() -> i64` and owns its entry checks, CPU
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
        let module = self.backend.compiler().compile(bytes).await?;
        self.dispatch
            .install(&mut self.backend, &self.bindings, &module, entries)
            .await
    }

    /// Borrows the stopped CPU and guest memory. Writes take effect immediately.
    pub fn context(&mut self) -> CpuContext<'_> {
        CpuContext::new(self.bindings.cpu(&mut self.backend), self.memory.view())
    }

    /// Shares guest memory through a handle that can outlive this runtime.
    /// Native handles can move to another thread; browser handles preserve their
    /// shared allocation during worker transport. CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory<B::SharedMemory> {
        self.memory.clone()
    }
}
