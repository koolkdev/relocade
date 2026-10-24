use super::{state::PendingCall, Call, MachineCore, RuntimeState};
use crate::{CpuConfig, CpuView, HookResult, MachineConfig, MemoryView};
use relocade_wasm_runtime::{MemoryAccess, StateAccess, WasmContext};
use relocade_x86::{ExecutionExit, ExecutionTarget};
use std::{
    cell::{Ref, RefCell, RefMut},
    future::Future,
    rc::Rc,
};

#[cfg(test)]
mod tests;

/// Scoped access to a stopped CPU, its guest memory and environment state.
/// CPU and memory edits apply directly to their live storage. Drop this context
/// before guest execution or polling an owned host call.
pub struct CpuContext<'a, T: 'static = ()> {
    source: Source<'a, T>,
    data: Rc<RefCell<T>>,
}

enum Source<'a, T: 'static> {
    Callback {
        cpu: &'a CpuConfig,
        config: &'a MachineConfig,
        context: MemoryAccess<'a>,
        guest: MemoryAccess<'a>,
        state: StateAccess<'a, RuntimeState<T>>,
    },
    Stopped(RefMut<'a, MachineCore<T>>),
}

impl<'a, T: 'static> CpuContext<'a, T> {
    #[inline]
    pub(super) fn new(
        cpu: &'a CpuConfig,
        config: &'a MachineConfig,
        context: MemoryAccess<'a>,
        guest: MemoryAccess<'a>,
        state: impl Into<StateAccess<'a, RuntimeState<T>>>,
    ) -> Self {
        let state = state.into();
        let data = Rc::clone(&state.data);
        Self {
            source: Source::Callback {
                cpu,
                config,
                context,
                guest,
                state,
            },
            data,
        }
    }

    pub(super) fn stopped(core: RefMut<'a, MachineCore<T>>, data: Rc<RefCell<T>>) -> Self {
        Self {
            source: Source::Stopped(core),
            data,
        }
    }

    pub fn execution_target(&self) -> ExecutionTarget {
        match &self.source {
            Source::Callback { cpu, .. } => cpu.execution_target(),
            Source::Stopped(core) => core.bindings.config.execution_target(),
        }
    }

    #[inline]
    pub fn cpu(&mut self) -> CpuView<'_> {
        match &mut self.source {
            Source::Callback { cpu, context, .. } => CpuView::new(cpu, context.reborrow()),
            Source::Stopped(core) => core.cpu(),
        }
    }

    #[inline]
    pub fn memory(&mut self) -> MemoryView<'_> {
        match &mut self.source {
            Source::Callback { config, guest, .. } => MemoryView::new(config, guest.reborrow()),
            Source::Stopped(core) => core.memory(),
        }
    }

    /// Borrows environment data only for the lifetime of this guard.
    pub fn state(&self) -> Ref<'_, T> {
        self.data.borrow()
    }

    pub fn state_mut(&mut self) -> RefMut<'_, T> {
        self.data.borrow_mut()
    }

    /// Queues an owned async operation after the synchronous guest call returns.
    /// Return the result from the hook. The future receives a [`Call`] whose
    /// methods borrow live state only while they run. It survives dropping a
    /// `run` waiter; explicit cancellation or CPU destruction drops it.
    /// Prefer [`super::Hook::asynchronous`] for a recurring service: registered
    /// handlers reuse typed future storage instead of boxing every invocation.
    pub fn defer<F, Fut>(&mut self, handler: F) -> HookResult
    where
        F: FnOnce(Call<T>) -> Fut + 'static,
        Fut: Future<Output = HookResult> + 'static,
    {
        self.queue(PendingCall::Once(Box::new(handler)))
    }

    pub(super) fn queue_registered(&mut self, id: usize) -> HookResult {
        self.queue(PendingCall::Registered(id))
    }

    fn queue(&mut self, pending: PendingCall<T>) -> HookResult {
        match &mut self.source {
            Source::Callback { state, .. } => {
                assert!(state.pending.is_none(), "host call already deferred");
                state.pending = Some(pending);
            }
            Source::Stopped(core) => {
                let mut state = core.backend.state_mut();
                assert!(state.pending.is_none(), "host call already deferred");
                state.pending = Some(pending);
            }
        }
        HookResult::Exit(ExecutionExit::Deferred)
    }
}
