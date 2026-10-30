//! Invocation access validation and independently retained service resources.

use super::{machine::Machine, CpuContext};
use crate::{CpuAccess, CpuView, IoBus, MemoryBus};
use relocade_wasm_runtime::{MemoryAccess, WasmMemoryAccess};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::{Rc, Weak},
};

pub(super) struct Access<T> {
    machine: Weak<RefCell<Machine<T>>>,
    pub active: Cell<bool>,
    interrupt_scopes: Cell<usize>,
    restore_interrupts: Cell<bool>,
}

/// Owned resources and CPU access for an asynchronous [`super::Hook::new`].
///
/// Each invocation retains its memory and I/O bus independently of the backend.
/// `call.cpu.ah()` and `call.cpu.set_ax(value)` use the same register operations
/// as a borrowed [`CpuView`], validating access and releasing the backend borrow
/// before returning. CPU access is valid only in this invocation's future poll
/// or completion cleanup. `call.memory.view()` borrows the retained [`MemoryBus`]
/// independently of `call.cpu` and may cross awaits. Capture service handles for
/// asynchronous work; use [`Self::with`] for arbitrary mutable environment state
/// and synchronous batches.
///
/// ```
/// # use relocade_machine::{CallContext, MemoryError};
/// async fn read_input(mut call: CallContext, input: impl std::future::Future<Output = u16>)
///     -> Result<(), MemoryError>
/// {
///     let mut memory = call.memory.view();
///     let value = input.await;
///     call.cpu.set_ax(value);
///     memory.write_u16(0x200, value)
/// }
/// ```
///
/// ```compile_fail
/// # use relocade_machine::CallContext;
/// async fn retain_cpu(call: CallContext<u32>) {
///     let state = call.with(|call| call.state);
///     std::future::pending::<()>().await;
///     *state = 1;
/// }
/// ```
pub struct CallContext<T = ()> {
    access: Weak<Access<T>>,
    pub cpu: CpuView<InvocationAccess<T>>,
    pub memory: MemoryBus,
    pub io: IoBus,
}

/// Access to one invocation's registers, valid only while its future is polled.
/// Retaining this handle does not retain the backend or the invocation.
pub struct InvocationAccess<T = ()> {
    access: Weak<Access<T>>,
}

impl<T> InvocationAccess<T> {
    fn access(&self) -> Rc<Access<T>> {
        let access = self.access.upgrade().expect("call is still alive");
        assert!(access.active.get(), "CPU is polling this call");
        access
    }
}

impl<T> CpuAccess for InvocationAccess<T> {
    fn with<R>(&self, operation: impl FnOnce(&MemoryAccess<'_>) -> R) -> R {
        let access = self.access();
        let machine = access.machine.upgrade().expect("CPU is still alive");
        let mut machine = machine.borrow_mut();
        let machine = &mut *machine;
        let storage = machine.backend.memory(&machine.bindings.context);
        operation(&storage)
    }

    fn with_mut<R>(&mut self, operation: impl FnOnce(&mut MemoryAccess<'_>) -> R) -> R {
        let access = self.access();
        let machine = access.machine.upgrade().expect("CPU is still alive");
        let mut machine = machine.borrow_mut();
        let machine = &mut *machine;
        let mut storage = machine.backend.memory(&machine.bindings.context);
        operation(&mut storage)
    }
}

impl<T> CallContext<T> {
    /// Borrows the live CPU, memory and environment for a synchronous operation.
    /// Return owned values; references to the machine cannot outlive the callback.
    /// Panics outside this invocation's future poll or completion cleanup.
    pub fn with<R>(&self, operation: impl FnOnce(&mut CpuContext<'_, T>) -> R) -> R {
        let access = self.access.upgrade().expect("call is still alive");
        assert!(access.active.get(), "CPU is polling this call");
        let machine = access.machine.upgrade().expect("CPU is still alive");
        let mut machine = machine.borrow_mut();
        let mut context = machine.context();
        operation(&mut context)
    }

    /// Enables live IF for the duration of an await.
    ///
    /// Every pending await allows IRQ delivery according to IF; this helper only
    /// manages the flag. Use an ordinary await if IF is already set as required.
    ///
    /// Restores the previous live IF on completion or cancellation, preserving
    /// all other flags and the caller's saved interrupt frame. Guest IRQ return
    /// must reach the installed resume entry before this future is polled again.
    /// Abandoning a parked invocation never restores IF into the active frame.
    ///
    /// Nested and overlapping wrappers restore IF only when the last scope ends.
    /// Dropping a run waiter or exhausting an ISR's instruction budget preserves
    /// the future and its IF scope.
    pub async fn with_interrupts_enabled<F: Future>(&self, future: F) -> F::Output {
        let access = self.access.upgrade().expect("call is still alive");
        self.with(|call| {
            if access.interrupt_scopes.get() == 0 {
                access.restore_interrupts.set(call.cpu.interrupts_enabled());
            }
            call.cpu.set_interrupts_enabled(true);
        });
        access
            .interrupt_scopes
            .set(access.interrupt_scopes.get() + 1);
        let _scope = InterruptsEnabled(access);
        future.await
    }
}

struct InterruptsEnabled<T>(Rc<Access<T>>);

impl<T> Drop for InterruptsEnabled<T> {
    fn drop(&mut self) {
        let remaining = self.0.interrupt_scopes.get() - 1;
        self.0.interrupt_scopes.set(remaining);
        if remaining == 0 && self.0.active.get() {
            // An abandoned parked call must not edit the active guest frame.
            if let Some(machine) = self.0.machine.upgrade() {
                machine
                    .borrow_mut()
                    .context()
                    .cpu
                    .set_interrupts_enabled(self.0.restore_interrupts.get());
            }
        }
    }
}

impl<T> Access<T> {
    pub fn new(machine: &Rc<RefCell<Machine<T>>>) -> (Rc<Self>, CallContext<T>) {
        let access = Rc::new(Self {
            machine: Rc::downgrade(machine),
            active: Cell::new(false),
            interrupt_scopes: Cell::new(0),
            restore_interrupts: Cell::new(false),
        });
        let context = {
            let machine = machine.borrow();
            CallContext {
                access: Rc::downgrade(&access),
                cpu: CpuView::new(
                    &machine.bindings.config,
                    InvocationAccess {
                        access: Rc::downgrade(&access),
                    },
                ),
                memory: machine.bindings.guest.clone().with_io(machine.io.clone()),
                io: machine.io.clone(),
            }
        };
        (access, context)
    }
}
