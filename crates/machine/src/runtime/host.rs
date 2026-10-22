use crate::CpuContext;
use relocade_x86::ExecutionExit;
use std::{future::Future, pin::Pin};

/// The next action after a host handler has edited the stopped CPU.
pub enum HostAction {
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return to the embedding. `Deferred` must instead carry an owned operation.
    Exit(ExecutionExit),
    /// Wait for an owned operation before entering guest code again.
    Await(HostOperation),
}

impl HostAction {
    /// Transfers an asynchronous operation to the CPU.
    ///
    /// The operation owns its captures and receives fresh access to the stopped
    /// CPU and memory. Views may stay borrowed across awaits. Neither the captures
    /// nor the future need to be Send. Returning another wait continues the same
    /// instruction budget without entering guest code between operations.
    pub fn wait<F>(operation: F) -> Self
    where
        F: AsyncFnOnce(&mut CpuContext<'_>) -> Self + 'static,
    {
        Self::Await(HostOperation(Box::new(operation)))
    }
}

/// An owned host operation constructed by [`HostAction::wait`].
/// The CPU retains it until an asynchronous run awaits it or cancellation drops it.
pub struct HostOperation(Box<dyn Operation>);

impl HostOperation {
    pub(super) async fn run(self, context: &mut CpuContext<'_>) -> HostAction {
        self.0.run(context).await
    }
}

trait Operation {
    fn run<'a>(
        self: Box<Self>,
        context: &'a mut CpuContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HostAction> + 'a>>;
}

impl<F> Operation for F
where
    F: AsyncFnOnce(&mut CpuContext<'_>) -> HostAction + 'static,
{
    fn run<'a>(
        self: Box<Self>,
        context: &'a mut CpuContext<'_>,
    ) -> Pin<Box<dyn Future<Output = HostAction> + 'a>> {
        Box::pin(async move { (*self)(context).await })
    }
}
