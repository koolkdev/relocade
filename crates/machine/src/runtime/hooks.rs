use relocade_compiler::{abi::Function, Program};
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit};

use super::{
    deferred::{AsyncHandler, Handler as AsyncFactory},
    Call, Cpu, CpuContext, DispatchEntry, ResourceOwner, Result,
};
use std::future::Future;

/// Result of a synchronous or deferred host hook.
/// The same type crosses generated hook entries through the shared Wasm ABI.
#[derive(
    relocade_wasm_bindings::WasmAbi,
    relocade_compiler::abi::CompilerValue,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum HookResult {
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return to Rust with an execution exit.
    Exit(ExecutionExit),
}

type SyncHandler<T> = dyn Fn(&mut CpuContext<'_, T>) -> HookResult + Send + Sync;

enum Handler<T: 'static> {
    Sync(Box<SyncHandler<T>>),
    Async(Box<dyn AsyncHandler<T>>),
}

/// A host function reached at a guest linear address through ordinary dispatch.
/// `T` is the backend's environment state, borrowed through the caller.
///
/// ```
/// use relocade_machine::{Hook, HookResult, CpuContext};
/// use relocade_x86::ExecutionExit;
///
/// fn terminate(call: &mut CpuContext<'_, Option<u8>>) -> HookResult {
///     let code = call.cpu().al();
///     *call.state_mut() = Some(code);
///     HookResult::Exit(ExecutionExit::HostReturn)
/// }
/// let hook = Hook::new(0xf0100, terminate);
/// ```
pub struct Hook<T: 'static = ()> {
    address: u32,
    handler: Handler<T>,
}

impl<T: 'static> Hook<T> {
    /// On Continue, the handler must set the guest continuation in CPU state.
    /// Exit returns its execution exit without changing that state.
    /// To defer work, return the result of [`CpuContext::defer`], which queues the
    /// required handler. Returning `ExecutionExit::Deferred` without a queued
    /// handler is a programming error.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Captures must be Send + Sync. Mutable environment data belongs in the
    /// caller's state, which does not need to be Send or Sync.
    pub fn new(
        address: u32,
        handler: impl Fn(&mut CpuContext<'_, T>) -> HookResult + Send + Sync + 'static,
    ) -> Self {
        Self {
            address,
            handler: Handler::Sync(Box::new(handler)),
        }
    }

    /// Registers an ordinary async service. The guest call returns to Rust before
    /// the future is polled. Ready services finish within that same run poll.
    /// One pinned slot is reused per simultaneous invocation; captures and futures
    /// may contain worker-local values and need not implement Send or Sync.
    pub fn asynchronous<F, Fut>(address: u32, handler: F) -> Self
    where
        F: Fn(Call<T>) -> Fut + 'static,
        Fut: Future<Output = HookResult> + 'static,
    {
        Self {
            address,
            handler: Handler::Async(Box::new(AsyncFactory::new(handler))),
        }
    }
}

impl<T: 'static> Cpu<T> {
    /// Generates and installs ordinary callable entries, each with its own direct
    /// host import. Dispatch neither classifies hooks nor calls a hook registry.
    ///
    /// Entries begin with CPU state already published by the predecessor. They
    /// cache no CPU values across the host call. Hooks do not retire guest
    /// instructions; continuing execution retains the current instruction target.
    /// Handlers must not reenter guest execution. They may use `CpuContext::defer`
    /// to await owned work and complete in Rust before guest execution resumes.
    // Linking exclusively owns the backend; no service is polled during setup.
    #[allow(clippy::await_holding_refcell_ref)]
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook<T>>) -> Result<()> {
        let hooks: Vec<_> = hooks.into_iter().collect();
        let names: Vec<_> = (0..hooks.len())
            .map(|index| format!("hook_{index}"))
            .collect();
        let mut program = Program::new();
        let dispatch = abi::DISPATCH.import(&mut program);
        let mut imports = Vec::new();
        let mut entries = Vec::new();
        for (hook, name) in hooks.into_iter().zip(&names) {
            let declaration: Function<fn() -> HookResult> = Function::new("hooks", name);
            let import = declaration.import(&mut program);
            let function = program.function::<abi::ExecutionSignature>(move |mut body, ()| {
                import
                    .call(&mut body, ())?
                    .match_(&mut body, |body, result| match result {
                        HookResultCase::Continue => dispatch.tail_call(body, ()),
                        HookResultCase::Exit(exit) => body.return_(exit),
                    })?;
                body.trap()
            });
            program.export(name, function)?;
            let handler = match hook.handler {
                Handler::Sync(handler) => handler,
                Handler::Async(handler) => {
                    let id = self.handlers.len();
                    self.handlers.push(handler);
                    Box::new(move |context: &mut CpuContext<'_, T>| context.queue_registered(id))
                }
            };
            let mut core = self.core.borrow_mut();
            let resources = ResourceOwner::clone(&core.bindings);
            imports.push(core.backend.bind_host(
                declaration,
                Box::new(move |mut caller| handler(&mut resources.context(&mut caller))),
            ));
            entries.push(DispatchEntry {
                address: hook.address,
                export: name,
            });
        }
        let bytes = program.compile()?;
        let compiler = self.core.borrow().backend.compiler();
        let module = compiler.compile(&bytes).await?;
        let core = &mut *self.core.borrow_mut();
        core.dispatch
            .install(
                &mut core.backend,
                &core.bindings,
                &module,
                &imports,
                &entries,
            )
            .await
    }
}
