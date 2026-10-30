//! Execution of one guest, independent of the host Wasm engine.
//!
//! [`Cpu::new`] prepares an interpreter and dispatcher over supplied [`Memory`]
//! and initializes private CPU storage. [`Cpu::with`] lends scoped access to
//! the stopped CPU and mapped memory, both backed directly by Wasm storage.
//! CPU edits take effect immediately. Preparation is asynchronous
//! for native and web engines. [`Cpu::run`] awaits host operations
//! between synchronous Wasm calls.
//!
//! The CPU keeps its machine in one allocation and retains each pending host call.
//! [`CallContext`] owns invocation access and retained memory/I/O resources. Every pending host await
//! allows IRQ delivery according to live IF. The same Rust future and local values
//! remain suspended while guest IRQ handlers execute, resuming only through
//! the service's own guest resume label. Environment state is
//! independent of backend state.
//!
//! A pending call survives dropping its run waiter. Dropping the CPU ends
//! the call chain and destroys the machine without undoing completed effects.
//! A pending host call excludes embedding access to CPU and environment state;
//! independent input handles and shared guest memory remain accessible. A returned
//! ISR budget or fault exit permits CPU access while keeping its call futures.
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
//!     let ax = cpu.with(|call| call.cpu.ax());
//!     # let _ = (exit, ax);
//!     Ok(())
//! }
//! ```
#![forbid(unsafe_code)]

mod access;
mod call;
mod code;
mod context;
mod dispatch;
mod entries;
mod error;
mod execution;
mod hooks;
mod io;
mod machine;
mod memory;
pub mod real_mode_interrupt;

use crate::{CpuConfig, IoBus, Memory};
pub use access::{CallContext, InvocationAccess};
use call::Call;
pub use context::CpuContext;
use dispatch::Dispatch;
pub use dispatch::DispatchEntry;
pub use error::{Error, Result};
use execution::Frame;
pub use hooks::{Hook, HookResult};
use machine::Machine;
use memory::RuntimeMemory;
use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
    rc::Rc,
};

type Backend = relocade_wasm_runtime::Backend<IoBus>;

/// Executes one guest and owns its machine, registered hooks and suspended calls.
pub struct Cpu<T: 'static = ()> {
    machine: Rc<RefCell<Machine<T>>>,
    active_call: Option<Frame<T>>,
    suspended_calls: Vec<Frame<T>>,
    memory: Memory,
}

impl<T: 'static> Cpu<T> {
    /// Creates private CPU state and dispatch over existing guest memory.
    /// Guest bytes are preserved. Use `()` when no environment state is needed.
    /// The environment state stays with this CPU, including during host calls.
    /// Port and MMIO accesses use the supplied execution-local device bus.
    pub async fn new(memory: Memory, cpu: CpuConfig, io: IoBus, state: T) -> Result<Self> {
        let mut backend = Backend::with_state(io.clone())?;
        let bindings = RuntimeMemory::new(&mut backend, memory.clone(), cpu)?;
        let host_imports = io::imports(&mut backend);
        let dispatch = Dispatch::new(&mut backend, &bindings, host_imports).await?;
        let machine = Machine {
            backend,
            bindings,
            io,
            state,
            dispatch,
            hooks: HashMap::new(),
        };
        Ok(Self {
            machine: Rc::new(RefCell::new(machine)),
            active_call: None,
            suspended_calls: Vec::new(),
            memory,
        })
    }

    /// Borrows the stopped machine for a synchronous operation.
    /// Return owned values; CPU, memory and environment borrows cannot escape
    /// the callback or cross an await.
    ///
    /// ```compile_fail
    /// # use relocade_machine::Cpu;
    /// fn escape(cpu: &mut Cpu<u32>) -> &mut u32 {
    ///     cpu.with(|call| call.state)
    /// }
    /// ```
    ///
    /// # Panics
    /// Panics while a host call is pending. Returned guest execution
    /// exits permit access even when interrupted calls remain suspended.
    pub fn with<R>(&mut self, operation: impl FnOnce(&mut CpuContext<'_, T>) -> R) -> R {
        let mut machine = self.machine_mut();
        let mut context = machine.context();
        operation(&mut context)
    }

    /// Retains guest memory independently of this CPU and its execution borrow.
    /// Clones share guest bytes; CPU and dispatch storage remain private.
    pub fn memory(&self) -> Memory {
        self.memory.clone()
    }

    fn machine_mut(&mut self) -> RefMut<'_, Machine<T>> {
        assert!(
            self.active_call.is_none(),
            "host call is suspended; complete it before accessing the machine"
        );
        self.machine.borrow_mut()
    }
}
