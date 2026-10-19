use relocade_compiler::{abi::Function, Program, I1};
use relocade_x86::{abi, ExecutionExit};
use std::{ops::ControlFlow, sync::Arc};

use crate::{with_host_call, DispatchEntry, HostCall, Result, Runtime, RuntimeBackend};

type Handler<T> = dyn Fn(&mut HostCall<'_, T>) -> ControlFlow<()> + Send + Sync;

/// A host function reached at a guest linear address through ordinary dispatch.
/// `T` is the backend's environment state, borrowed through the caller.
///
/// ```
/// use std::ops::ControlFlow;
/// use relocade_runtime::{Hook, HostCall};
///
/// fn terminate(call: &mut HostCall<'_, Option<u8>>) -> ControlFlow<()> {
///     let code = call.cpu().al();
///     *call.state_mut() = Some(code);
///     ControlFlow::Break(())
/// }
/// let hook = Hook::new(0xf0100, terminate);
/// ```
pub struct Hook<T = ()> {
    address: u32,
    handler: Box<Handler<T>>,
}

impl<T> Hook<T> {
    /// On Continue, the handler must set the guest continuation in CPU state.
    /// Break returns `ExecutionExit::HostReturn` without changing that state.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Captures must be Send + Sync. Mutable environment data belongs in the
    /// caller's state, which does not need to be Send or Sync.
    pub fn new(
        address: u32,
        handler: impl Fn(&mut HostCall<'_, T>) -> ControlFlow<()> + Send + Sync + 'static,
    ) -> Self {
        Self {
            address,
            handler: Box::new(handler),
        }
    }
}

impl<B: RuntimeBackend> Runtime<B> {
    /// Generates and installs ordinary callable entries, each with its own direct
    /// host import. Dispatch neither classifies hooks nor calls a hook registry.
    ///
    /// Entries begin with CPU state already published by the predecessor. They
    /// cache no CPU values across the host call. Hooks do not retire guest
    /// instructions; continuing execution retains the current instruction target.
    /// Handlers are synchronous and must not reenter guest execution.
    pub async fn install_hooks(
        &mut self,
        hooks: impl IntoIterator<Item = Hook<B::State>>,
    ) -> Result<()> {
        let hooks: Vec<_> = hooks.into_iter().collect();
        let names: Vec<_> = (0..hooks.len())
            .map(|index| format!("hook_{index}"))
            .collect();
        let mut program = Program::new();
        let dispatch = program.import_function(abi::DISPATCH);
        let mut imports = Vec::new();
        let mut entries = Vec::new();
        for (hook, name) in hooks.into_iter().zip(&names) {
            let declaration: Function<fn() -> I1> = Function::new("hooks", name);
            let import = program.import_function(declaration);
            let function = program.function(abi::RUN.signature(), |mut body| {
                let resume = body.call::<I1>(import, &[])?;
                body.if_(resume.eq(false), |stop| {
                    stop.return_(ExecutionExit::HostReturn.to_bits())
                })?;
                body.tail_call(dispatch, &[])
            })?;
            program.export(name, function)?;
            let config = Arc::clone(&self.config);
            let context = self.storage.context.clone();
            let guest = self.storage.guest.clone();
            imports.push(self.storage.access.bind_host(
                declaration,
                Box::new(move |mut caller| {
                    let flow = with_host_call(
                        &config,
                        &mut caller,
                        context.clone(),
                        guest.clone(),
                        |call| (hook.handler)(call),
                    );
                    i32::from(flow.is_continue())
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
