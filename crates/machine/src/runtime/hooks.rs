use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit, ExecutionExitValues};

use super::{Cpu, CpuContext, DispatchEntry, Result};

/// Result of a synchronous or deferred host hook.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookResult {
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return an execution exit to the run loop. `ExecutionExit::HostCall` is
    /// reserved for generated entries and must not be returned by a handler.
    Exit(ExecutionExit),
}

pub(super) type Handler<T> = dyn Fn(&mut CpuContext<'_, T>) -> HookResult;

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
pub struct Hook<T = ()> {
    address: u32,
    handler: Box<Handler<T>>,
}

impl<T> Hook<T> {
    /// On Continue, the handler must set the guest continuation in CPU state.
    /// Exit returns its execution exit without changing that state.
    /// To defer work, return the result of [`CpuContext::defer`], which queues the
    /// required handler. Returning `ExecutionExit::Deferred` without a queued
    /// handler is a programming error.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    pub fn new(
        address: u32,
        handler: impl Fn(&mut CpuContext<'_, T>) -> HookResult + 'static,
    ) -> Self {
        Self {
            address,
            handler: Box::new(handler),
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
    /// Handlers must not reenter guest execution. They may use `CpuContext::defer`
    /// to await owned work and complete in Rust before guest execution resumes.
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
        let module = self.backend.compiler().compile(&bytes).await?;
        self.dispatch
            .install(&mut self.backend, &self.bindings, &module, &entries)
            .await?;
        // Entry addresses also identify their handlers, including replacements.
        // Publish only after the module and its callable entries are installed.
        self.hooks.extend(handlers);
        Ok(())
    }
}
