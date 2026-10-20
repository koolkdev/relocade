use crate::{state::Completion, storage::Storage, wasm::WasmContext, RuntimeState};
use relocade_machine::{Cpu, MachineConfig, MachineStorage, Memory};
use std::{future::Future, ops::ControlFlow};

#[cfg(test)]
mod tests;

/// Access to CPU, guest memory and environment state for a host handler or its
/// deferred completion. This view borrows existing storage; edits apply immediately.
pub struct HostCall<'a, T = ()> {
    config: &'a MachineConfig,
    storage: &'a mut dyn HostStorage<T>,
}

impl<T> HostCall<'_, T> {
    /// Borrows the CPU that entered this host call.
    pub fn cpu(&mut self) -> Cpu<'_> {
        Cpu::new(self.config, self.storage)
    }

    /// Borrows the machine's shared guest memory through its configured map.
    pub fn memory(&mut self) -> Memory<'_> {
        Memory::new(self.config, self.storage)
    }

    /// The environment state owned by this backend.
    pub fn state(&self) -> &T {
        &self.storage.state().data
    }

    pub fn state_mut(&mut self) -> &mut T {
        &mut self.storage.state_mut().data
    }

    /// Suspends this hook until owned work completes outside Wasm.
    ///
    /// Use this from an installed hook or its completion. Return this method's
    /// Break to suspend guest execution. The future must own its inputs: it cannot
    /// borrow this call's CPU, memory or state. Neither the future nor its
    /// completion needs Send.
    ///
    /// `Runtime::run_async` polls the future, then invokes `complete` once with
    /// fresh access to the calling CPU and memory. The completion owns the ABI
    /// return, just like a synchronous hook, and may defer further work.
    pub fn defer<F, R, C>(&mut self, future: F, complete: C) -> ControlFlow<()>
    where
        T: 'static,
        F: Future<Output = R> + 'static,
        R: 'static,
        C: FnOnce(&mut HostCall<'_, T>, R) -> ControlFlow<()> + 'static,
    {
        assert!(
            self.storage.state().pending.is_none(),
            "host call already deferred"
        );
        self.storage.state_mut().pending = Some(Box::pin(async move {
            let result = future.await;
            Box::new(move |call: &mut HostCall<'_, T>| complete(call, result)) as Completion<T>
        }));
        ControlFlow::Break(())
    }
}

// Erase the backend's scoped access type while retaining its typed environment state.
trait HostStorage<T>: MachineStorage {
    fn state(&self) -> &RuntimeState<T>;
    fn state_mut(&mut self) -> &mut RuntimeState<T>;
}

impl<T, C: WasmContext<State = RuntimeState<T>>> HostStorage<T> for Storage<&mut C> {
    fn state(&self) -> &RuntimeState<T> {
        self.access.state()
    }

    fn state_mut(&mut self) -> &mut RuntimeState<T> {
        self.access.state_mut()
    }
}

/// Lends CPU, guest memory and environment state to a host handler or completion,
/// using the same live views as `Runtime::machine`.
///
/// The binding supplies the calling CPU's execution-context memory, the machine
/// configuration and guest memory. The access view can come from a host callback
/// or `WasmBackend::with_context` while stopped. Memory handles are moved into this
/// call; their contents stay in the engine. Guest code must publish CPU state
/// before access and observe changes afterward.
/// Ordinary port/MMIO imports do not provide that boundary. Guest reentry is
/// unsupported. Segment edits and snapshot restores validate before writing.
///
/// Returns the handler's result directly. Earlier CPU and memory edits remain
/// applied if the handler returns an error.
pub fn with_host_call<T, C: WasmContext<State = RuntimeState<T>>, R>(
    config: &MachineConfig,
    access: &mut C,
    context: C::Memory,
    guest: C::Memory,
    handler: impl FnOnce(&mut HostCall<'_, T>) -> R,
) -> R {
    let mut storage = Storage {
        access,
        context,
        guest,
    };
    handler(&mut HostCall {
        config,
        storage: &mut storage,
    })
}
