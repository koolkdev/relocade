use super::WakeTarget;
use crate::{
    worker::{ResourceTransfer, WorkerResource},
    Result,
};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    task::{Wake, Waker},
};

/// Keeps the host end of a wake connection alive. Native resources can be
/// accessed directly by the sending thread, so no host activity is needed.
pub struct WakeHost;

/// A session connection that can be transferred with `ThreadExt::run_resource`.
pub struct WakeEndpoint<R>(Arc<Mutex<Option<R>>>);

/// The execution end of a session's wake connection.
pub struct WakeConnection<R> {
    target: Arc<Mutex<Option<R>>>,
    notification: Waker,
    polling: super::Polling,
}

/// Connects a host and execution worker before either starts busy execution.
pub fn wake_connection<R: WakeTarget + Send>() -> Result<(WakeHost, WakeEndpoint<R>)> {
    Ok((WakeHost, WakeEndpoint(Arc::new(Mutex::new(None)))))
}

impl<R: WakeTarget + Send> WorkerResource for WakeEndpoint<R> {
    type Posted = ();

    fn prepare(self) -> ResourceTransfer<(), impl FnOnce(()) -> Self + Send + 'static> {
        ResourceTransfer::new((), move |()| self)
    }
}

impl<R: WakeTarget + Send> WakeEndpoint<R> {
    pub fn bind(self) -> WakeConnection<R> {
        let notification = Waker::from(Arc::new(Request(self.0.clone())));
        WakeConnection {
            target: self.0,
            notification,
            polling: super::Polling::default(),
        }
    }
}

struct Request<R>(Arc<Mutex<Option<R>>>);
impl<R: WakeTarget + Send> Wake for Request<R> {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        if let Some(target) = &*self.0.lock().unwrap() {
            target.wake();
        }
    }
}

impl<R: WakeTarget + Send> WakeConnection<R> {
    /// Installs the resource before entering busy execution. Replacing a CPU
    /// replaces the notification target for this session.
    pub async fn activate(&self, target: R) -> Result<()> {
        let previous = self.target.lock().unwrap().replace(target);
        drop(previous);
        // Abortable registers only after Pending, before busy execution starts.
        self.yield_now().await;
        Ok(())
    }

    /// Polls the entire session, including cancellation, with notifying wakers.
    pub async fn run<F: Future>(&self, future: F) -> F::Output {
        self.polling
            .run(future, &self.notification, || {
                if let Some(target) = &*self.target.lock().unwrap() {
                    target.acknowledge();
                }
            })
            .await
    }

    /// Yields busy execution without requesting another interrupt. The current
    /// task's waker still runs, including any enclosing combinator's bookkeeping.
    pub async fn yield_now(&self) {
        self.polling.yield_now().await;
    }
}

impl<R> Drop for WakeConnection<R> {
    fn drop(&mut self) {
        let target = self.target.lock().unwrap().take();
        drop(target);
    }
}
