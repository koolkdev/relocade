use crate::{CpuConfig, CpuView, HookResult, MachineConfig, MemoryView, RuntimeState};
use relocade_wasm_runtime::{MemoryAccess, StateAccess};
use relocade_x86::{ExecutionExit, ExecutionTarget};

/// Scoped access to a stopped CPU, its guest memory and environment state.
/// `Cpu::context`, synchronous hooks and deferred handlers all lend this same
/// capability. CPU and memory edits apply directly to their live storage.
pub struct CpuContext<'a, T = ()> {
    cpu: &'a CpuConfig,
    config: &'a MachineConfig,
    context: MemoryAccess<'a>,
    guest: MemoryAccess<'a>,
    state: StateAccess<'a, RuntimeState<T>>,
}

impl<'a, T> CpuContext<'a, T> {
    /// Binds an initialized `ExecutionContext` image and guest backing. Their
    /// contents and capacities must match the configurations; CPU state must
    /// satisfy the execution target. CPU edits check compatibility before writing.
    #[inline]
    pub fn new(
        cpu: &'a CpuConfig,
        config: &'a MachineConfig,
        context: MemoryAccess<'a>,
        guest: MemoryAccess<'a>,
        state: impl Into<StateAccess<'a, RuntimeState<T>>>,
    ) -> Self {
        Self {
            cpu,
            config,
            context,
            guest,
            state: state.into(),
        }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        self.cpu.execution_target()
    }

    #[inline]
    pub fn cpu(&mut self) -> CpuView<'_> {
        CpuView::new(self.cpu, self.context.reborrow())
    }

    #[inline]
    pub fn memory(&mut self) -> MemoryView<'_> {
        MemoryView::new(self.config, self.guest.reborrow())
    }

    /// The environment state owned by this backend.
    #[inline]
    pub fn state(&self) -> &T {
        &self.state.data
    }

    #[inline]
    pub fn state_mut(&mut self) -> &mut T {
        &mut self.state.data
    }

    /// Queues an async handler for `Cpu::run_async` after any guest call returns.
    ///
    /// Return this method's `HookResult::Exit(ExecutionExit::Deferred)` from the hook to
    /// suspend guest execution.
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
        assert!(self.state.pending.is_none(), "host call already deferred");
        self.state.pending = Some(Box::new(handler));
        HookResult::Exit(ExecutionExit::Deferred)
    }
}
