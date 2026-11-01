use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExitValues, HostExitValues};

use super::{Backend, Cpu, Dispatch, DispatchEntry, Result, RuntimeMemory};
use futures_util::future::LocalBoxFuture;
use std::{
    collections::HashMap,
    future::Future,
    rc::Rc,
    task::{Context, Poll},
};

pub(super) trait Handler {
    fn start(&self, cx: &mut Context<'_>) -> Option<LocalBoxFuture<'_, ()>>;
}

impl<F> Handler for F
where
    F: AsyncFn(),
{
    fn start(&self, cx: &mut Context<'_>) -> Option<LocalBoxFuture<'_, ()>> {
        // Pin before the first poll: locals may already borrow one another when
        // this future first suspends. Erase its type only if it remains pending.
        let mut future = Box::pin(self());
        match future.as_mut().poll(cx) {
            Poll::Ready(()) => None,
            Poll::Pending => Some(future),
        }
    }
}

/// A host service reached at a guest entry. Its ordinary async handler captures
/// CPU state, memory, I/O and application resources once at registration.
/// No backend borrow or access binding is held across a pending await.
/// Restore temporary CPU changes on normal completion, explicitly after the await.
/// Destructors release resources on abandonment; they must not restore an old
/// invocation's CPU state into the currently executing guest or another call.
///
/// ```
/// use relocade_machine::{CpuStateAccess, Hook, Memory, StopHandle};
/// fn service(cpu: CpuStateAccess, memory: Memory, stop: StopHandle) -> Hook {
///     Hook::new(0xf0100, async move || {
///         let value = std::future::ready(42).await;
///         cpu.set_ax(value);
///         memory.view().write_u16(0x200, value).unwrap();
///         stop.stop();
///     })
/// }
/// ```
///
pub struct Hook {
    address: u32,
    handler: Rc<dyn Handler>,
}

impl Hook {
    /// Normal completion continues guest execution at the live CPU position.
    /// Interrupt/ABI return conventions belong to the handler's environment;
    /// [`super::real_mode_interrupt::hook`] applies the interrupt return convention.
    /// A captured [`super::StopHandle`] can request that the current run end.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    pub fn new<F>(address: u32, handler: F) -> Self
    where
        F: AsyncFn() + 'static,
    {
        Self {
            address,
            handler: Rc::new(handler),
        }
    }
}

/// Owns stable service IDs and their live handlers. Address lookup is confined
/// to registration; execution indexes the handler directly by its generated ID.
#[derive(Default)]
pub(super) struct Services {
    ids: HashMap<u32, u32>,
    handlers: Vec<Option<Rc<dyn Handler>>>,
}

impl Services {
    #[inline]
    pub(super) fn handler(&self, id: u32) -> &Rc<dyn Handler> {
        self.handlers
            .get(id as usize)
            .and_then(Option::as_ref)
            .expect("registered host service")
    }

    /// Releases a replaced handler while retaining its ID for reinstallation.
    pub(super) fn remove(&mut self, address: u32) {
        if let Some(&id) = self.ids.get(&address) {
            self.handlers[id as usize] = None;
        }
    }

    async fn install(
        &mut self,
        backend: &mut Backend,
        bindings: &RuntimeMemory,
        dispatch: &mut Dispatch,
        hooks: impl IntoIterator<Item = Hook>,
    ) -> Result<()> {
        let hooks: Vec<_> = hooks.into_iter().collect();
        let names: Vec<_> = (0..hooks.len())
            .map(|index| format!("hook_{index}"))
            .collect();
        let mut program = Program::new();
        let mut handlers = Vec::new();
        // Registration owns address-to-ID lookup; executing a service indexes
        // its slot directly. Plan replacements before publishing any changes.
        let mut ids = self.ids.clone();
        let mut next_id = self.handlers.len();
        let mut entries = Vec::new();
        for (hook, name) in hooks.into_iter().zip(&names) {
            let address = hook.address;
            let service_id = *ids.entry(address).or_insert_with(|| {
                let id = u32::try_from(next_id).expect("host service ID capacity");
                next_id += 1;
                id
            });
            let function = program.function::<abi::ExecutionSignature>(move |body, ()| {
                body.return_(ExecutionExitValues::host(HostExitValues::call(service_id)))
            });
            program.export(name, function)?;
            handlers.push((service_id, hook.handler));
            entries.push(DispatchEntry {
                address,
                export: name,
            });
        }
        let bytes = program.compile()?;
        let compiler = backend.compiler();
        let module = compiler.compile(&bytes).await?;
        dispatch
            .install(backend, bindings, &module, &entries)
            .await?;
        // Publish after all exports are installed. Removed entries retain their
        // ID for later reinstallation; obsolete handler captures are released.
        self.handlers.resize_with(next_id, || None);
        for (id, handler) in handlers {
            self.handlers[id as usize] = Some(handler);
        }
        self.ids = ids;
        Ok(())
    }
}

impl Cpu {
    /// Installs guest entries carrying a stable service ID to the Rust run loop.
    /// The CPU owns the handlers and invokes them after the guest call returns.
    ///
    /// Entries begin with CPU state already published by the predecessor. They
    /// cache no CPU values across the host call. Hooks do not retire guest
    /// instructions; continuing execution retains the current instruction target.
    /// The driver polls handlers only after guest execution returns. Pending calls
    /// retain their locals and captured resources; ready awaits continue in the
    /// same poll. Dropping a run waiter preserves that call.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it before installing.
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook>) -> Result<()> {
        let machine = self.machine_mut();
        machine
            .services
            .install(
                &mut machine.backend,
                &machine.bindings,
                &mut machine.dispatch,
                hooks,
            )
            .await
    }
}
