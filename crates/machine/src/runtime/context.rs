//! Borrowed access to the machine executing a host service.

use super::machine::Machine;
use crate::{CpuView, HookResult, MemoryView};
use relocade_wasm_runtime::{WasmMemoryAccess, WasmSharedMemory};
use relocade_x86::{ExecutionExit, ExecutionTarget, HostExit};

/// Exclusive access to a stopped machine and its environment.
/// CPU and memory views borrow live storage and apply writes immediately.
/// A view can cross an ordinary await, keeping its machine borrow until it ends.
/// Contexts cannot escape their handler or the borrow of [`super::Cpu`].
///
/// A live view excludes another run:
/// ```compile_fail
/// use relocade_machine::Cpu;
/// async fn overlap(cpu: &mut Cpu) {
///     let mut context = cpu.context();
///     let view = context.cpu();
///     cpu.run(1);
///     let _ = view.ax();
/// }
/// ```
pub struct CpuContext<'a, T = ()> {
    pub(super) machine: &'a mut Machine<T>,
}

impl<T> CpuContext<'_, T> {
    pub fn execution_target(&self) -> ExecutionTarget {
        self.machine.bindings.config.execution_target()
    }

    #[inline]
    pub fn cpu(&mut self) -> CpuView<'_> {
        CpuView::new(
            &self.machine.bindings.config,
            self.machine.backend.memory(&self.machine.bindings.context),
        )
    }

    #[inline]
    pub fn memory(&mut self) -> MemoryView<'_> {
        MemoryView::new(
            &self.machine.bindings.machine_config,
            self.machine.bindings.shared_guest.view(),
        )
    }

    pub fn state(&self) -> &T {
        &self.machine.state
    }

    pub fn state_mut(&mut self) -> &mut T {
        &mut self.machine.state
    }

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
        assert!(self.machine.pending.is_none(), "host call already deferred");
        self.machine.pending = Some(Box::new(handler));
        HookResult::Exit(ExecutionExit::Host(HostExit::Deferred))
    }
}
