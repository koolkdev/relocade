use crate::{worker::Worker, Application, Context};
use arc_swap::ArcSwapOption;
use futures_channel::mpsc;
use relocade_machine::TimerHandle;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context as TaskContext, Poll},
};

/// Starts execution on the existing machine worker infrastructure. The embedding
/// must drive the timer service associated with `clock` while execution is alive.
pub fn spawn<A: Application>(application: A, clock: TimerHandle) -> Task<A> {
    let source = Arc::new(ArcSwapOption::empty());
    let (input, receiver) = mpsc::unbounded();
    let context = Context {
        clock,
        input: receiver,
        source: source.clone(),
    };
    Task {
        worker: Worker::spawn((), move |()| application.run(context)),
        source,
        input,
    }
}

/// Execution lifetime and communication with its owner. `stop` requests cancellation;
/// await the task before releasing it so worker-local Rust state is dropped first.
/// Completion returns `Some(output)`, cancellation `None`, and failures an error.
#[must_use = "retain and await the application task through completion or cancellation"]
pub struct Task<A: Application> {
    worker: Worker<Result<A::Output, String>>,
    pub(crate) source: Arc<ArcSwapOption<A::Source>>,
    input: mpsc::UnboundedSender<A::Input>,
}
impl<A: Application> Task<A> {
    /// Returns false when execution no longer accepts input.
    pub fn send_input(&self, input: A::Input) -> bool {
        self.input.unbounded_send(input).is_ok()
    }
    pub fn current_source(&self) -> Option<Arc<A::Source>> {
        self.source.load_full()
    }
    pub fn stop(&self) {
        self.worker.stop();
    }
}
impl<A: Application> Future for Task<A> {
    type Output = Result<Option<A::Output>, String>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        let result = std::task::ready!(self.worker.poll(cx))?;
        Poll::Ready(result.transpose())
    }
}
