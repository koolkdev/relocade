//! Invocation access validation and independently retained service resources.

use super::{machine::Machine, CpuContext};
use crate::{CpuAccess, CpuView, IoBus, Memory, MemoryView};
use relocade_wasm_runtime::{MemoryAccess, WasmMemoryAccess};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

pub(super) struct Access<T> {
    machine: Weak<RefCell<Machine<T>>>,
    pub active: Cell<bool>,
}

/// Owned resources and CPU access for an asynchronous [`super::Hook::new`].
///
/// Each invocation retains its memory and I/O bus independently of the backend.
/// `call.cpu.ah()` and `call.cpu.set_ax(value)` use the same register operations
/// as a borrowed [`CpuView`], validating access and releasing the backend borrow
/// before returning. CPU access is valid only in this invocation's future poll
/// or completion cleanup. Memory views borrow the retained resources and may
/// cross awaits. Capture service handles for asynchronous work; use [`Self::with`]
/// for arbitrary mutable environment state and synchronous batches.
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
    pub memory: Memory,
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
    /// Guest memory and MMIO access borrows only the retained resources, never
    /// the backend. This view may remain alive across an await.
    pub fn memory(&self) -> MemoryView<'_> {
        self.memory.view().with_io(&self.io)
    }

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
}

impl<T> Access<T> {
    pub fn new(machine: &Rc<RefCell<Machine<T>>>) -> (Rc<Self>, CallContext<T>) {
        let access = Rc::new(Self {
            machine: Rc::downgrade(machine),
            active: Cell::new(false),
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
                memory: machine.bindings.guest.clone(),
                io: machine.io.clone(),
            }
        };
        (access, context)
    }
}
