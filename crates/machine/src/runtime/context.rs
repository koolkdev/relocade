//! Borrowed access to the machine executing a host service.

use super::callback::DeferredCall;
use crate::{CpuView, HookResult, MemoryView};
use relocade_x86::{ExecutionExit, HostExit};

/// Exclusive access to a stopped machine and its environment.
/// CPU and memory views borrow live storage and apply writes immediately.
/// The fields borrow separate resources and can be used together, including
/// while borrowing environment state. Borrow a field instead of the whole context.
/// A view can cross an ordinary await, keeping its machine borrow until it ends.
/// Contexts cannot escape their handler or the borrow of [`super::Cpu`].
/// Drop the context before resuming guest execution or installing modules.
///
/// A live view excludes another run:
/// ```compile_fail
/// use relocade_machine::Cpu;
/// async fn overlap(cpu: &mut Cpu) {
///     let mut context = cpu.context();
///     let view = &context.cpu;
///     cpu.run(1);
///     let _ = view.ax();
/// }
/// ```
pub struct CpuContext<'a, T = ()> {
    /// Live CPU registers, independent of guest memory and environment state.
    pub cpu: CpuView<'a>,
    /// Guest memory through the machine's physical map.
    pub memory: MemoryView<'a>,
    /// The environment state owned by this machine.
    pub state: &'a mut T,
    pub(super) pending: &'a mut Option<Box<dyn DeferredCall<T>>>,
}

impl<T> CpuContext<'_, T> {
    /// Queues an async handler for `Cpu::run_async` after any guest call returns.
    ///
    /// Return this method's result from the hook to suspend guest execution.
    /// It carries `ExecutionExit::Host(HostExit::Deferred)`.
    /// The handler must own its captures; it cannot capture this context's borrows.
    /// `Cpu::run_async` gives it fresh access to the stopped CPU, memory and
    /// state. That access may be held across awaits. The handler owns the ABI
    /// return, just like a synchronous hook, and need not be Send.
    ///
    /// Keep the same run future alive while waiting. Dropping it during the
    /// handler cancels the handler; see `Cpu::run_async`.
    pub fn defer<F>(&mut self, handler: F) -> HookResult
    where
        T: 'static,
        F: AsyncFnOnce(&mut CpuContext<'_, T>) -> HookResult + 'static,
    {
        assert!(self.pending.is_none(), "host call already deferred");
        *self.pending = Some(Box::new(handler));
        HookResult::Exit(ExecutionExit::Host(HostExit::Deferred))
    }
}
