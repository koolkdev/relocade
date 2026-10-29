use futures_util::future::{AbortHandle, Abortable};
use relocade_machine::worker::{Post, SendTask, Thread};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

// Retains the thread through cancellation and destruction of its local future.
pub(crate) struct Worker<T: Send> {
    thread: Option<Thread>,
    task: Option<SendTask<Option<T>>>,
    stop: AbortHandle,
}
impl<T: Send + 'static> Worker<T> {
    pub fn spawn<C: Post + 'static, F: Future<Output = T> + 'static>(
        context: C,
        run: impl FnOnce(C) -> F + Send + 'static,
    ) -> Self {
        let thread = Thread::new();
        let (stop, registration) = AbortHandle::new_pair();
        let task = thread.run_send(context, move |context| async move {
            Abortable::new(run(context), registration).await.ok()
        });
        Self {
            thread: Some(thread),
            task: Some(task),
            stop,
        }
    }
    pub fn stop(&self) {
        self.stop.abort();
    }
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Result<Option<T>, String>> {
        let Some(task) = &mut self.task else {
            return Poll::Pending;
        };
        let result = std::task::ready!(Pin::new(task).poll(cx));
        self.task = None;
        self.thread = None;
        Poll::Ready(result.map_err(|error| error.to_string()))
    }
}
impl<T: Send> Drop for Worker<T> {
    fn drop(&mut self) {
        self.stop.abort();
    }
}
