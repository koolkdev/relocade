use crate::{CpuContext, CpuView, HookResult, MemoryView, RuntimeState};
use relocade_x86::ExecutionExit;
use std::{future::Future, pin::Pin};

/// Access to CPU, guest memory and environment state for a host handler.
/// Only `Cpu::run` and `Cpu::run_async` lend this view to installed hooks and
/// deferred handlers. It borrows live CPU and memory; edits apply immediately.
pub struct HostCall<'a, T = ()> {
    context: CpuContext<'a>,
    state: &'a mut RuntimeState<T>,
}

impl<'a, T> HostCall<'a, T> {
    pub(crate) fn new(context: CpuContext<'a>, state: &'a mut RuntimeState<T>) -> Self {
        assert!(state.running, "host handler requires an active CPU run");
        Self { context, state }
    }

    /// Borrows the CPU that entered this host call.
    #[inline]
    pub fn cpu(&mut self) -> CpuView<'_> {
        self.context.cpu()
    }

    /// Borrows the machine's shared guest memory through its configured map.
    #[inline]
    pub fn memory(&mut self) -> MemoryView<'_> {
        self.context.memory()
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

    /// Defers an async handler until the generated guest call has returned.
    ///
    /// Return this method's `HookResult::Exit(ExecutionExit::Deferred)` from the hook to
    /// suspend guest execution.
    /// The handler must own its captures; it cannot capture this call's borrows.
    /// `Cpu::run_async` gives it fresh access to the stopped CPU, memory and
    /// state. That access may be held across awaits. The handler owns the ABI
    /// return, just like a synchronous hook, and need not be Send.
    ///
    /// Keep the same run future alive while waiting. Dropping it during the
    /// handler cancels the handler; see `Cpu::run_async`.
    pub fn defer<F>(&mut self, handler: F) -> HookResult
    where
        T: 'static,
        F: AsyncFnOnce(&mut HostCall<'_, T>) -> HookResult + 'static,
    {
        assert!(self.state.pending.is_none(), "host call already deferred");
        self.state.pending = Some(Box::new(handler));
        HookResult::Exit(ExecutionExit::Deferred)
    }
}

/// Erases the handler while allowing its future to borrow the stopped host call.
pub(crate) trait DeferredCall<T> {
    fn run<'a>(
        self: Box<Self>,
        call: &'a mut HostCall<'_, T>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + 'a>>;
}

impl<T, F> DeferredCall<T> for F
where
    F: AsyncFnOnce(&mut HostCall<'_, T>) -> HookResult + 'static,
{
    fn run<'a>(
        self: Box<Self>,
        call: &'a mut HostCall<'_, T>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + 'a>> {
        Box::pin(async move { (*self)(call).await })
    }
}
