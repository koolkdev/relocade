use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit, ExecutionExitValues, HostExitValues};

use super::{CallContext, Cpu, DispatchEntry, Result};
use futures_util::future::LocalBoxFuture;
use std::rc::Rc;

pub(super) trait Handler<T> {
    fn call(self: Rc<Self>, context: CallContext<T>) -> LocalBoxFuture<'static, HookResult>;
}

impl<T: 'static, F> Handler<T> for F
where
    F: AsyncFn(CallContext<T>) -> HookResult + 'static,
{
    fn call(self: Rc<Self>, context: CallContext<T>) -> LocalBoxFuture<'static, HookResult> {
        Box::pin(async move { self(context).await })
    }
}

/// Result of a completed host call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookResult {
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return an execution exit to the run loop. `HostExit::Call`
    /// and `InterruptInhibited` are reserved for generated entries and must not
    /// be returned by a handler.
    Exit(ExecutionExit),
}

/// A host function reached at a guest linear address through ordinary dispatch.
/// `T` is the machine's environment state, borrowed through the context.
///
/// ```
/// use relocade_machine::{CallContext, Hook, HookResult};
/// use relocade_x86::{ExecutionExit, HostExit};
///
/// async fn terminate(call: CallContext<Option<u8>>) -> HookResult {
///     call.with(|context| *context.state = Some(context.cpu.al()));
///     HookResult::Exit(ExecutionExit::Host(HostExit::Return))
/// }
/// let hook = Hook::new(0xf0100, terminate);
/// ```
pub struct Hook<T = ()> {
    address: u32,
    handler: Rc<dyn Handler<T>>,
}

impl<T: 'static> Hook<T> {
    /// Creates a host call with invocation CPU access and retained memory/I/O resources.
    /// Its future and locals survive waiter changes. Ready awaits continue
    /// synchronously; pending awaits finish before guest dispatch resumes.
    /// On Continue, set the guest continuation. Exit preserves CPU state.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    ///
    /// Handlers run in Rust after guest Wasm returns. Captures and environment data
    /// stay on the CPU's thread and do not need Send or Sync.
    pub fn new<F>(address: u32, handler: F) -> Self
    where
        F: AsyncFn(CallContext<T>) -> HookResult + 'static,
    {
        Self {
            address,
            handler: Rc::new(handler),
        }
    }
}

impl<T: 'static> Cpu<T> {
    /// Installs callable entries that return their address to the Rust run loop.
    /// The CPU owns the handlers and invokes them after the guest call returns.
    ///
    /// Entries begin with CPU state already published by the predecessor. They
    /// cache no CPU values across the host call. Hooks do not retire guest
    /// instructions; continuing execution retains the current instruction target.
    /// Each call retains its future; register operations borrow only while accessing
    /// storage. `CallContext::with` supports synchronous batches. Dropping a run waiter
    /// preserves the pending call.
    ///
    /// # Panics
    /// Panics while a host call is pending.
    #[allow(
        clippy::await_holding_refcell_ref,
        reason = "Installation excludes guest execution and pending host calls."
    )]
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook<T>>) -> Result<()> {
        let mut borrow = self.machine_mut();
        let machine = &mut *borrow;
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
