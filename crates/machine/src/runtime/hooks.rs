use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit, ExecutionExitValues};

use super::{
    calls::{Handler, Service},
    Cpu, CpuContext, DispatchEntry, Result,
};
use std::{cell::RefCell, future::Future, rc::Rc};

/// Result of a completed host service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookResult {
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return an execution exit to the run loop. `ExecutionExit::HostCall` is
    /// reserved for generated entries and must not be returned by a handler.
    Exit(ExecutionExit),
}

/// A host function reached at a guest linear address through ordinary dispatch.
/// `T` is the machine's environment state, accessed through the owned context.
///
/// ```
/// use relocade_machine::{Hook, HookResult, CpuContext};
/// use relocade_x86::ExecutionExit;
///
/// async fn terminate(call: CpuContext<Option<u8>>) -> HookResult {
///     let code = call.cpu().al();
///     call.with_state_mut(|state| *state = Some(code));
///     HookResult::Exit(ExecutionExit::HostReturn)
/// }
/// let hook = Hook::new(0xf0100, terminate);
/// ```
pub struct Hook<T = ()> {
    address: u32,
    handler: Rc<RefCell<dyn Handler<T>>>,
}

impl<T: 'static> Hook<T> {
    /// On Continue, the handler must set the guest continuation in CPU state.
    /// Exit returns its execution exit without changing that state.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    pub fn new<F, Fut>(address: u32, handler: F) -> Self
    where
        F: Fn(CpuContext<T>) -> Fut + 'static,
        Fut: Future<Output = HookResult> + 'static,
    {
        Self {
            address,
            handler: Rc::new(RefCell::new(Service::new(handler))),
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
    /// Handlers may await owned work and complete in Rust before guest execution
    /// resumes. The runner owns each future until completion or explicit cancellation.
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook<T>>) -> Result<()> {
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
                body.return_(ExecutionExitValues::host_call(address))
            });
            program.export(name, function)?;
            handlers.push((address, hook.handler));
            entries.push(DispatchEntry {
                address,
                export: name,
            });
        }
        let bytes = program.compile()?;
        let compiler = self.core.backend.borrow().compiler();
        let module = compiler.compile(&bytes).await?;
        self.dispatch
            .install(&self.core.backend, &self.core.bindings, &module, &entries)
            .await?;
        // Entry addresses also identify their handlers, including replacements.
        // Publish only after the module and its callable entries are installed.
        self.hooks.extend(handlers);
        Ok(())
    }
}
