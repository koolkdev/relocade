use relocade_compiler::Program;
use relocade_wasm_runtime::{WasmBackend, WasmCompiler};
use relocade_x86::{abi, ExecutionExit, ExecutionExitValues, HostExitValues};

use super::{Cpu, CpuContext, DispatchEntry, Result};
use futures_util::future::LocalBoxFuture;
use std::rc::Rc;

pub(super) trait Handler {
    fn call<'a>(&'a self, context: &'a mut CpuContext<'_>) -> LocalBoxFuture<'a, HookResult>;
}

impl<F> Handler for F
where
    F: AsyncFn(&mut CpuContext<'_>) -> HookResult,
{
    fn call<'a>(&'a self, context: &'a mut CpuContext<'_>) -> LocalBoxFuture<'a, HookResult> {
        Box::pin(self(context))
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

/// A host function reached at a guest linear address through ordinary dispatch.
/// Application state belongs to the service's captures.
///
/// ```
/// use relocade_machine::{Hook, HookResult};
/// use relocade_x86::{ExecutionExit, HostExit};
///
/// let exit_code = std::rc::Rc::new(std::cell::Cell::new(None));
/// let hook = Hook::new(0xf0100, async move |call| {
///     exit_code.set(Some(call.cpu.al()));
///     HookResult::Exit(ExecutionExit::Host(HostExit::Return))
/// });
/// ```
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
        F: AsyncFn(&mut CpuContext<'_>) -> HookResult + 'static,
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
    /// Handlers borrow the machine across ordinary awaits and complete in Rust
    /// before guest execution resumes. Dropping a run waiter preserves that call.
    ///
    /// # Panics
    /// Panics if a service is suspended. Complete it before installing.
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook>) -> Result<()> {
        let machine = self.execution.machine_mut();
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
