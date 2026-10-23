use crate::{runtime::access::CpuAccess, HookResult, RuntimeMemory, RuntimeState};
use crate::{CpuConfig, CpuStorage, CpuView, MachineConfig, MemoryView};
use relocade_wasm_runtime::WasmContext;
use relocade_x86::ExecutionExit;
use std::{future::Future, pin::Pin};

#[cfg(test)]
mod tests;

/// Access to CPU, guest memory and environment state for a host handler.
/// This view borrows live CPU and memory; edits apply immediately.
pub struct HostCall<'a, T = ()> {
    cpu: &'a CpuConfig,
    config: &'a MachineConfig,
    access: &'a mut dyn HostAccess<T>,
}

impl<'a, T> HostCall<'a, T> {
    pub(crate) fn new(
        cpu: &'a CpuConfig,
        config: &'a MachineConfig,
        access: &'a mut dyn HostAccess<T>,
    ) -> Self {
        Self {
            cpu,
            config,
            access,
        }
    }

    /// Borrows the CPU that entered this host call.
    pub fn cpu(&mut self) -> CpuView<'_> {
        CpuView::new(self.cpu, self.access)
    }

    /// Borrows the machine's shared guest memory through its configured map.
    pub fn memory(&mut self) -> MemoryView<'_> {
        MemoryView::new(self.config, self.access)
    }

    /// The environment state owned by this backend.
    pub fn state(&self) -> &T {
        &self.access.state().data
    }

    pub fn state_mut(&mut self) -> &mut T {
        &mut self.access.state_mut().data
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
        assert!(
            self.access.state().pending.is_none(),
            "host call already deferred"
        );
        self.access.state_mut().pending = Some(Box::new(handler));
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

// Erase the backend's scoped access type while retaining its typed environment state.
pub(crate) trait HostAccess<T>: CpuStorage {
    fn state(&self) -> &RuntimeState<T>;
    fn state_mut(&mut self) -> &mut RuntimeState<T>;
}

impl<T, C: WasmContext<State = RuntimeState<T>>> HostAccess<T> for CpuAccess<&mut C> {
    fn state(&self) -> &RuntimeState<T> {
        self.backend.state()
    }

    fn state_mut(&mut self) -> &mut RuntimeState<T> {
        self.backend.state_mut()
    }
}

impl<M: Clone + Send + Sync + 'static> RuntimeMemory<M> {
    /// Lends the published CPU, guest memory and environment state
    /// to a synchronous host handler. The caller must publish CPU state before
    /// access and observe edits afterward. Ordinary port/MMIO imports do not
    /// provide this boundary. Reentering this CPU from its callback is unsupported.
    /// Earlier edits remain applied if the handler returns an error.
    pub fn with_host_call<T, C: WasmContext<State = RuntimeState<T>, Memory = M>, R>(
        &self,
        context: &mut C,
        handler: impl FnOnce(&mut HostCall<'_, T>) -> R,
    ) -> R {
        let mut access = CpuAccess {
            backend: context,
            bindings: self.clone(),
        };
        handler(&mut HostCall::new(
            &self.config,
            &self.machine_config,
            &mut access,
        ))
    }
}
