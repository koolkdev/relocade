use relocade_compiler::{abi::Function, Program, I64};
use relocade_x86::{abi, ExecutionExit};
use std::ops::ControlFlow;

use super::{Cpu, DispatchEntry, HostCall, Result, RuntimeBackend, RuntimeState};

type Handler<T> = dyn Fn(&mut HostCall<'_, T>) -> ControlFlow<ExecutionExit> + Send + Sync;

/// A host function reached at a guest linear address through ordinary dispatch.
/// `T` is the backend's environment state, borrowed through the caller.
///
/// ```
/// use std::ops::ControlFlow;
/// use relocade_machine::{Hook, HostCall};
/// use relocade_x86::ExecutionExit;
///
/// fn terminate(call: &mut HostCall<'_, Option<u8>>) -> ControlFlow<ExecutionExit> {
///     let code = call.cpu().al();
///     *call.state_mut() = Some(code);
///     ControlFlow::Break(ExecutionExit::HostReturn)
/// }
/// let hook = Hook::new(0xf0100, terminate);
/// ```
pub struct Hook<T = ()> {
    address: u32,
    handler: Box<Handler<T>>,
}

impl<T> Hook<T> {
    /// On Continue, the handler must set the guest continuation in CPU state.
    /// Break returns its execution exit without changing that state.
    /// To defer work, return the result of [`HostCall::defer`], which queues the
    /// required handler. Returning `ExecutionExit::Deferred` without a queued
    /// handler is a programming error.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Captures must be Send + Sync. Mutable environment data belongs in the
    /// caller's state, which does not need to be Send or Sync.
    pub fn new(
        address: u32,
        handler: impl Fn(&mut HostCall<'_, T>) -> ControlFlow<ExecutionExit> + Send + Sync + 'static,
    ) -> Self {
        Self {
            address,
            handler: Box::new(handler),
        }
    }
}

impl<T: 'static, B: RuntimeBackend<State = RuntimeState<T>>> Cpu<B> {
    /// Generates and installs ordinary callable entries, each with its own direct
    /// host import. Dispatch neither classifies hooks nor calls a hook registry.
    ///
    /// Entries begin with CPU state already published by the predecessor. They
    /// cache no CPU values across the host call. Hooks do not retire guest
    /// instructions; continuing execution retains the current instruction target.
    /// Handlers must not reenter guest execution. They may use `HostCall::defer`
    /// to await owned work and complete in Rust before guest execution resumes.
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook<T>>) -> Result<()> {
        let hooks: Vec<_> = hooks.into_iter().collect();
        let names: Vec<_> = (0..hooks.len())
            .map(|index| format!("hook_{index}"))
            .collect();
        let mut program = Program::new();
        let dispatch = program.import_function(abi::DISPATCH);
        let mut imports = Vec::new();
        let mut entries = Vec::new();
        for (hook, name) in hooks.into_iter().zip(&names) {
            let declaration: Function<fn() -> I64> = Function::new("hooks", name);
            let import = program.import_function(declaration);
            let function = program.function(abi::RUN.signature(), |mut body| {
                let exit = body.call::<I64>(import, &[])?;
                body.if_(exit.ne(0), |stop| stop.return_(exit))?;
                body.tail_call(dispatch, &[])
            })?;
            program.export(name, function)?;
            let memory = self.storage.memory.clone();
            imports.push(self.storage.access.bind_host(
                declaration,
                Box::new(move |mut caller| {
                    let flow = memory.with_host_call(&mut caller, |call| (hook.handler)(call));
                    match flow {
                        ControlFlow::Continue(()) => 0,
                        ControlFlow::Break(exit) => exit.to_bits() as i64,
                    }
                }),
            ));
            entries.push(DispatchEntry {
                address: hook.address,
                export: name,
            });
        }
        let instance = self.instantiate(&program.compile()?, &imports).await?;
        self.dispatch_table
            .install(&mut self.storage.access, &instance, &entries)
    }
}
