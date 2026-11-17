use super::closed;
use async_channel::{Receiver, Sender};
use futures_channel::{mpsc, oneshot};
use futures_util::future::LocalBoxFuture;
use std::{fmt, io};

type Call<T> = Box<dyn for<'a> FnOnce(&'a mut T) -> LocalBoxFuture<'a, ()> + Send>;

/// Registers resources with the service that owns and polls them.
#[derive(Clone)]
pub(crate) struct Resources {
    pub(super) tasks: mpsc::UnboundedSender<LocalBoxFuture<'static, ()>>,
}

impl Resources {
    pub(crate) fn register<T: 'static>(&self, mut value: T) -> io::Result<Resource<T>> {
        let (calls, pending) = async_channel::bounded(1);
        let pending = PendingCalls(pending);
        self.tasks
            .unbounded_send(Box::pin(async move {
                while let Ok(call) = pending.0.recv().await {
                    call(&mut value).await;
                }
            }))
            .map_err(|_| closed())?;
        Ok(Resource { calls })
    }
}

/// Only the closures and results cross threads. The resource and the futures
/// borrowing it are created/polled on the service's owner and need not be Send.
pub(crate) struct Resource<T> {
    calls: Sender<Call<T>>,
}

impl<T: 'static> Resource<T> {
    pub(crate) async fn call<R: Send + 'static>(
        &self,
        operation: impl AsyncFnOnce(&mut T) -> io::Result<R> + Send + 'static,
    ) -> io::Result<R> {
        let (reply, result) = oneshot::channel();
        self.calls
            .send(Box::new(move |value| {
                Box::pin(async move {
                    let _ = reply.send(operation(value).await);
                })
            }))
            .await
            .map_err(|_| closed())?;
        result.await.map_err(|_| closed())?
    }
}

impl<T> Clone for Resource<T> {
    fn clone(&self) -> Self {
        Self {
            calls: self.calls.clone(),
        }
    }
}

impl<T> fmt::Debug for Resource<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_struct("Resource").finish_non_exhaustive()
    }
}

struct PendingCalls<T>(Receiver<Call<T>>);

impl<T> Drop for PendingCalls<T> {
    fn drop(&mut self) {
        // Closing async-channel alone retains buffered calls while senders live.
        // Discard them so their reply senders release every waiting caller.
        self.0.close();
        while self.0.try_recv().is_ok() {}
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
