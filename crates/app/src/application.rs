use crate::{task::Task, Exit};
use futures_util::future::AbortHandle;
use std::future::Future;

/// Sendable launch data. Execution state is created by `run` on its worker.
pub trait Application: Send + Sized + 'static {
    /// Application-defined messages sent to its embedding host.
    type Update: Send + 'static;
    type Output: Send + 'static;

    /// The future stays on the application worker and need not be Send.
    /// Always-ready execution must check `Context::is_stopping` between slices.
    fn run(
        self,
        context: Context<Self::Update>,
    ) -> impl Future<Output = Result<Exit<Self::Output>, String>>;
}

/// Publishes application updates and observes the host's cancellation request.
pub struct Context<U> {
    stop: AbortHandle,
    notify: Box<dyn Fn(U) + Send>,
}

impl<U> Context<U> {
    pub fn send(&self, update: U) {
        (self.notify)(update);
    }

    pub fn is_stopping(&self) -> bool {
        self.stop.is_aborted()
    }
}

/// Starts an application task without a window. Await it before dropping it.
/// A windowed host uses this same path and forwards updates to its event loop.
pub fn spawn<A: Application>(
    application: A,
    notify: impl Fn(A::Update) + Send + 'static,
) -> Task<A::Output> {
    Task::spawn((), move |(), stop| {
        application.run(Context {
            stop,
            notify: Box::new(notify),
        })
    })
}
