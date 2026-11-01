use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit, ExecutionExitValues, HostExitValues};

use super::{Cpu, DispatchEntry, Result};
use futures_util::future::LocalBoxFuture;
use std::{
    future::Future,
    rc::Rc,
    task::{Context, Poll},
};

pub(super) enum Started<'call> {
    Ready(HookResult),
    Pending(LocalBoxFuture<'call, HookResult>),
}

pub(super) trait Handler {
    fn start(&self, cx: &mut Context<'_>) -> Started<'_>;
}

impl<F> Handler for F
where
    F: AsyncFn() -> HookResult,
{
    fn start(&self, cx: &mut Context<'_>) -> Started<'_> {
        // Pin before the first poll: locals may already borrow one another when
        // this future first suspends. Erase its type only if it remains pending.
        let mut future = Box::pin(self());
        match future.as_mut().poll(cx) {
            Poll::Ready(result) => Started::Ready(result),
            Poll::Pending => Started::Pending(future),
        }
    }
}

/// Result of a completed host service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookResult {
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return an execution exit to the run loop. `HostExit::Call` is
    /// reserved for generated entries and must not be returned by a handler.
    Exit(ExecutionExit),
}

/// A host service reached at a guest entry. Its ordinary async handler captures
/// CPU state, memory, I/O and application resources once at registration.
/// No backend borrow or access binding is held across a pending await.
/// Restore temporary CPU changes on normal completion, explicitly after the await.
/// Destructors release resources on abandonment; they must not restore an old
/// invocation's CPU state into the currently executing guest or another call.
///
/// ```
/// use relocade_machine::{CpuStateAccess, Hook, HookResult, Memory};
/// use relocade_x86::{ExecutionExit, HostExit};
/// fn service(cpu: CpuStateAccess, memory: Memory) -> Hook {
///     Hook::new(0xf0100, async move || {
///         let value = std::future::ready(42).await;
///         cpu.set_ax(value);
///         memory.view().write_u16(0x200, value).unwrap();
///         HookResult::Exit(ExecutionExit::Host(HostExit::Return))
///     })
/// }
/// ```
///
pub struct Hook {
    address: u32,
    handler: Rc<dyn Handler>,
}

impl Hook {
    /// On Continue, the handler must set the guest continuation in CPU state.
    /// Exit returns its execution exit without changing that state.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    pub fn new<F>(address: u32, handler: F) -> Self
    where
        F: AsyncFn() -> HookResult + 'static,
    {
        Self {
            address,
            handler: Rc::new(handler),
        }
    }
}

impl Cpu {
    /// Installs callable entries that return their address to the Rust run loop.
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
        let hooks: Vec<_> = hooks.into_iter().collect();
        let names: Vec<_> = (0..hooks.len())
            .map(|index| format!("hook_{index}"))
            .collect();
        let mut program = Program::new();
        let mut handlers = Vec::new();
        let mut entries = Vec::new();
        for (hook, name) in hooks.into_iter().zip(&names) {
            let address = hook.address;
            let function = program.function::<abi::ExecutionSignature>(move |body, ()| {
                body.return_(ExecutionExitValues::host(HostExitValues::call(address)))
            });
            program.export(name, function)?;
            handlers.push((address, hook.handler));
            entries.push(DispatchEntry {
                address,
                export: name,
            });
        }
        let bytes = program.compile()?;
        let compiler = machine.backend.compiler();
        let module = compiler.compile(&bytes).await?;
        machine
            .dispatch
            .install(&mut machine.backend, &machine.bindings, &module, &entries)
            .await?;
        // Entry addresses also identify their handlers, including replacements.
        // Publish only after the module and its callable entries are installed.
        machine.hooks.extend(handlers);
        Ok(())
    }
}
