//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher and initializes
//! backend storage from [`MachineConfig`]. [`Cpu::context`] returns owned access
//! to the stopped CPU and mapped memory, backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] awaits host operations
//! between synchronous Wasm calls.
//!
//! The machine core owns engine storage and environment data. The execution
//! runner owns the core, dispatch and pinned service futures. Services own their
//! local values and call context; each context operation validates its invocation
//! and ends its storage borrow before returning. Environment state is independent
//! of the backend's callback state.
//!
//! A pending service survives dropping its run waiter. Explicit cancellation
//! revokes its contexts before dropping the future, without undoing its effects.
//! The embedding must replace the cancelled guest continuation before resuming.
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

mod calls;
mod code;
mod context;
mod core;
mod dispatch;
mod error;
mod execution;
mod hooks;
mod io;
mod memory;
pub mod real_mode_interrupt;
#[cfg(feature = "worker")]
pub mod worker;

use crate::{CpuConfig, MachineConfig, Memory};
pub use context::CpuContext;
use core::MachineCore;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
pub use hooks::{Hook, HookResult};
use memory::RuntimeMemory;
use relocade_wasm_runtime::{SharedMemory, WasmBackend, WasmCompiler};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::{Deref, DerefMut},
    rc::Rc,
};

type Backend = relocade_wasm_runtime::Backend;

/// Executes one guest and owns its machine, registered services and suspended calls.
pub struct Cpu<T: 'static = ()> {
    core: Rc<MachineCore<T>>,
    dispatch: Dispatch,
    hooks: HashMap<u32, Rc<RefCell<dyn calls::Handler<T>>>>,
    pending: Option<calls::PendingCall<T>>,
    next_call: u64,
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
        let bindings = Rc::new(RuntimeMemory::new(&mut backend, config, cpu)?);
        let host_imports = io::unconnected_imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        Ok(Self {
            core: Rc::new(MachineCore {
                backend: RefCell::new(backend),
                bindings,
                state: RefCell::new(state),
                active_call: Cell::new(None),
                running: Cell::new(false),
                closed: Cell::new(false),
            }),
            dispatch,
            hooks: HashMap::new(),
            pending: None,
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
    pub async fn install_module(
        &mut self,
        bytes: &[u8],
        entries: &[DispatchEntry<'_>],
    ) -> Result<()> {
        let compiler = self.core.backend.borrow().compiler();
        let module = compiler.compile(bytes).await?;
        self.dispatch
            .install(&self.core.backend, &self.core.bindings, &module, entries)
            .await?;
        for entry in entries {
            self.hooks.remove(&entry.address);
        }
        Ok(())
    }

    /// Returns owned access to the stopped machine. Each operation borrows its
    /// storage briefly; retaining this handle does not prevent subsequent runs.
    /// Access after this CPU is dropped is a programming error.
    pub fn context(&self) -> CpuContext<T> {
        CpuContext::new(&self.core, None)
    }

    /// Shares guest memory through a handle that can outlive this runtime.
    /// Native handles can move to another thread; browser handles preserve their
    /// shared allocation during worker transport. CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory<SharedMemory> {
        Memory::with_backing(
            self.core.bindings.machine_config.clone(),
            self.core.bindings.shared_guest.clone(),
        )
        .expect("guest backing matches the configured capacity")
    }

    /// Borrows the environment state between guest calls. Drop the borrow before running.
    pub fn state(&self) -> impl Deref<Target = T> + '_ {
        self.core.state.borrow()
    }

    pub fn state_mut(&mut self) -> impl DerefMut<Target = T> + '_ {
        self.core.state.borrow_mut()
    }
}

impl<T: 'static> Drop for Cpu<T> {
    fn drop(&mut self) {
        // Revoke handles before dropping futures or user-owned captures.
        self.core.closed.set(true);
        self.core.active_call.set(None);
        self.pending = None;
        self.hooks.clear();
    }
}
