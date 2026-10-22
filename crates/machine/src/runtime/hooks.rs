use crate::{Cpu, CpuContext, HostAction, Result, RuntimeBackend};

pub(super) type Handler = dyn FnMut(&mut CpuContext<'_>) -> HostAction;

/// A Rust handler reached at a guest linear address through ordinary dispatch.
/// Its captures own the service handles it needs; they do not need Send or Sync.
///
/// ```
/// use std::{cell::Cell, rc::Rc};
/// use relocade_machine::{Hook, HostAction};
/// use relocade_x86::ExecutionExit;
///
/// let status = Rc::new(Cell::new(None));
/// let result = Rc::clone(&status);
/// let hook = Hook::new(0xf0100, move |context| {
///     result.set(Some(context.cpu().al()));
///     HostAction::Exit(ExecutionExit::HostReturn)
/// });
/// ```
pub struct Hook {
    pub(super) address: u32,
    pub(super) handler: Box<Handler>,
}

impl Hook {
    /// A continuing handler sets the guest continuation in CPU state. Interrupt
    /// and ABI return conventions belong to the service. To wait, return an owned
    /// operation from [`HostAction::wait`].
    pub fn new(
        address: u32,
        handler: impl FnMut(&mut CpuContext<'_>) -> HostAction + 'static,
    ) -> Self {
        Self {
            address,
            handler: Box::new(handler),
        }
    }
}

impl<B: RuntimeBackend> Cpu<B> {
    /// Installs address entries that return to the CPU's Rust execution loop.
    /// Handlers receive the predecessor's published CPU state. Hooks do not retire
    /// guest instructions; continuing and waiting retain the current target.
    /// Replacing an entry releases its previous handler. Already queued operations
    /// keep their own captures independently of hook replacement.
    pub async fn install_hooks(&mut self, hooks: impl IntoIterator<Item = Hook>) -> Result<()> {
        self.dispatch
            .install_hooks(&mut self.backend, &self.bindings, hooks)
            .await
    }
}
