pub(crate) mod requests;

use crate::{Error, Result};
use futures_channel::{mpsc, oneshot};
use futures_util::{future::Shared, FutureExt, StreamExt};
use std::{
    future::Future,
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex},
};

type Completion = Shared<futures_util::future::BoxFuture<'static, Result<()>>>;
type Ready = Arc<Mutex<Option<oneshot::Sender<Result<()>>>>>;

/// Owns submission to a native worker. Dropping it closes input without joining.
pub struct Worker<T> {
    input: Option<mpsc::UnboundedSender<T>>,
    ready: Completion,
    stopped: Completion,
}

/// The receiving half of a message channel. A cancelled receive consumes nothing.
pub struct Receiver<T>(mpsc::UnboundedReceiver<T>);

impl<T> Receiver<T> {
    pub async fn recv(&mut self) -> Option<T> {
        self.0.next().await
    }
}

/// Worker-owned command input and event output.
pub struct Endpoint<C, E> {
    input: Receiver<C>,
    events: mpsc::UnboundedSender<E>,
    ready: Ready,
}

impl<C, E> Endpoint<C, E> {
    pub fn ready(&self) {
        if let Some(ready) = self.ready.lock().unwrap().take() {
            let _ = ready.send(Ok(()));
        }
    }

    pub async fn recv(&mut self) -> Option<C> {
        self.input.recv().await
    }

    pub fn send(&self, event: E) -> Result<()> {
        self.events.unbounded_send(event).map_err(|_| Error::Closed)
    }
}

impl<T: Send + 'static> Worker<T> {
    /// Starts a persistent async entry on a dedicated thread. Its future and
    /// worker-local state need not be Send; only the entry closure and messages
    /// cross threads. The entry is responsible for draining input and returning.
    pub fn spawn<E, F, Fut>(name: &str, entry: F) -> Result<(Self, Receiver<E>)>
    where
        E: Send + 'static,
        F: FnOnce(Endpoint<T, E>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>>,
    {
        let (input, commands) = mpsc::unbounded();
        let (events, output) = mpsc::unbounded();
        let (ready, readiness) = oneshot::channel();
        let (stopped, completion) = oneshot::channel();
        let ready = Arc::new(Mutex::new(Some(ready)));
        let endpoint = Endpoint {
            input: Receiver(commands),
            events,
            ready: Arc::clone(&ready),
        };
        std::thread::Builder::new()
            .name(name.into())
            .spawn(move || {
                let mut result =
                    catch_unwind(AssertUnwindSafe(|| pollster::block_on(entry(endpoint))))
                        .unwrap_or_else(|panic| {
                            let message = panic.downcast_ref::<String>().cloned().or_else(|| {
                                panic
                                    .downcast_ref::<&str>()
                                    .map(|message| (*message).into())
                            });
                            Err(Error::Failed(
                                message.unwrap_or_else(|| "worker panicked".into()),
                            ))
                        });
                if let Some(ready) = ready.lock().unwrap().take() {
                    let error = result.clone().err().unwrap_or_else(|| {
                        Error::Failed("worker stopped before reporting readiness".into())
                    });
                    let _ = ready.send(Err(error.clone()));
                    result = Err(error);
                }
                let _ = stopped.send(result);
            })
            .map_err(|error| Error::Failed(error.to_string()))?;
        Ok((
            Self {
                input: Some(input),
                ready: completion_future(readiness),
                stopped: completion_future(completion),
            },
            Receiver(output),
        ))
    }
}

impl<T> Worker<T> {
    pub fn send(&self, message: T) -> Result<()> {
        self.input
            .as_ref()
            .ok_or(Error::Closed)?
            .unbounded_send(message)
            .map_err(|_| Error::Closed)
    }

    pub fn ready(&self) -> impl Future<Output = Result<()>> + Send + 'static {
        self.ready.clone()
    }

    /// Closes input. Accepted messages remain available to the entry.
    pub fn shutdown(&mut self) {
        self.input.take();
    }

    /// Waits for the entry to return, including after shutdown or owner drop.
    pub fn stopped(&self) -> impl Future<Output = Result<()>> + Send + 'static {
        self.stopped.clone()
    }
}

fn completion_future(receiver: oneshot::Receiver<Result<()>>) -> Completion {
    async move { receiver.await.unwrap_or(Err(Error::Closed)) }
        .boxed()
        .shared()
}
