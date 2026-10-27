use crate::Exit;
use futures_channel::oneshot;
use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

/// A clonable shutdown request, independent of the completion future.
#[derive(Clone)]
pub struct StopHandle(pub(crate) Arc<dyn Fn() + Send + Sync>);

impl StopHandle {
    pub fn stop(&self) {
        (self.0)();
    }
}

/// Completion after the window closes and both worker tasks finish.
pub struct Control<T> {
    pub(crate) stop: StopHandle,
    pub(crate) completion: oneshot::Receiver<Result<Exit<T>, String>>,
}

impl<T> Control<T> {
    pub fn stop(&self) {
        self.stop.stop();
    }

    /// Keep this handle when moving the control into an await or browser Promise.
    pub fn stop_handle(&self) -> StopHandle {
        self.stop.clone()
    }
}

impl<T> Future for Control<T> {
    type Output = Result<Exit<T>, String>;

    fn poll(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.completion).poll(context).map(|result| {
            result
                .map_err(|_| "The application closed without completing shutdown".to_owned())
                .and_then(|result| result)
        })
    }
}
