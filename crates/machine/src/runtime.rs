//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher and initializes
//! backend storage from [`MachineConfig`]. [`Cpu::context`] lends access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] can await host operations
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
//!     let exit = runtime.run(100).await?;
//!     let ax = runtime.context().cpu().ax();
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

mod call;
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

use crate::{CpuConfig, CpuView, MachineConfig, Memory, MemoryView};
pub use call::{Call, CallCpu, CallMemory};
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use hooks::{Hook, HookResult, HookResultCase, HookResultValues};
use memory::RuntimeMemory;
use relocade_wasm_runtime::{
    SharedMemory, WasmBackend, WasmCompiler, WasmMemoryAccess, WasmSharedMemory,
};
use state::RuntimeState;
#[cfg(target_arch = "wasm32")]
use std::rc::Rc as ResourceOwner;
#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc as ResourceOwner;
use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};

type Backend<T> = relocade_wasm_runtime::Backend<RuntimeState<T>>;

/// One guest CPU and its engine resources, executable dispatch and pending calls.
pub struct Cpu<T: 'static = ()> {
    core: Rc<RefCell<MachineCore<T>>>,
    data: Rc<RefCell<T>>,
    handlers: Vec<Box<dyn deferred::AsyncHandler<T>>>,
    calls: Vec<deferred::CallFrame>,
    next_call: u64,
}

/// Guest resources are borrowed separately from the futures that access them.
struct MachineCore<T: 'static> {
    backend: Backend<T>,
    bindings: ResourceOwner<RuntimeMemory>,
    dispatch: Dispatch<T>,
    active_call: Option<u64>,
}

impl<T: 'static> MachineCore<T> {
    fn cpu(&mut self) -> CpuView<'_> {
        CpuView::new(
            &self.bindings.config,
            self.backend.memory(&self.bindings.context),
        )
    }

    fn memory(&self) -> MemoryView<'_> {
        MemoryView::new(
            &self.bindings.machine_config,
            self.bindings.shared_guest.view(),
        )
    }
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
        let data = Rc::new(RefCell::new(state));
        let mut backend = Backend::with_state(RuntimeState::new(Rc::clone(&data)))?;
        let bindings = ResourceOwner::new(RuntimeMemory::new(&mut backend, config, cpu)?);
        let host_imports = io::unconnected_imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        Ok(Self {
            core: Rc::new(RefCell::new(MachineCore {
                backend,
                bindings,
                dispatch,
                active_call: None,
            })),
            data,
            handlers: Vec::new(),
            calls: Vec::new(),
            next_call: 0,
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
    // Linking exclusively owns the backend; no service is polled during setup.
    #[allow(clippy::await_holding_refcell_ref)]
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let compiler = self.core.borrow().backend.compiler();
        let module = compiler.compile(bytes).await?;
        let core = &mut *self.core.borrow_mut();
        core.dispatch
            .install(&mut core.backend, &core.bindings, &module, &[], entries)
            .await
    }

    /// Borrows the stopped CPU, guest memory and environment state. Writes take
    /// effect immediately. This context owns a borrow guard: drop it before
    /// running the CPU or obtaining another context. Use one named context for
    /// multiple accesses in the same expression.
    pub fn context(&mut self) -> CpuContext<'_, T> {
        CpuContext::stopped(self.core.borrow_mut(), Rc::clone(&self.data))
    }

    /// Shares guest memory through a handle that can outlive this runtime.
    /// Native handles can move to another thread; browser handles preserve their
    /// shared allocation during worker transport. CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory<SharedMemory> {
        let core = self.core.borrow();
        Memory::with_backing(
            core.bindings.machine_config.clone(),
            core.bindings.shared_guest.clone(),
        )
        .expect("guest backing matches the configured capacity")
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> Ref<'_, T> {
        self.data.borrow()
    }

    pub fn state_mut(&mut self) -> RefMut<'_, T> {
        self.data.borrow_mut()
    }
}

impl<T: 'static> Drop for Cpu<T> {
    fn drop(&mut self) {
        self.cancel_pending_call();
    }
}
