use super::{WakeConnection, WakeTarget};
use crate::{
    worker::{ResourceTransfer, WorkerResource},
    Result,
};
use std::{
    sync::{Arc, Mutex},
    task::{Wake, Waker},
};

/// Keeps the host end of a wake connection alive. Native resources can be
/// accessed directly by the sending thread, so no host activity is needed.
pub struct WakeHost;

/// A session connection that can be transferred with `ThreadExt::run_resource`.
pub struct WakeEndpoint<R>(Arc<Mutex<Option<R>>>);

/// Retains cross-thread access until the execution connection is dropped.
pub(super) struct Binding<R>(Arc<Mutex<Option<R>>>);

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
        WakeConnection::new(Binding(self.0), notification)
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

impl<R> Binding<R> {
    pub(super) async fn install(&self, target: R) -> Result<()> {
        let previous = self.0.lock().unwrap().replace(target);
        drop(previous);
        Ok(())
    }
}

impl<R> Drop for Binding<R> {
    fn drop(&mut self) {
        let target = self.0.lock().unwrap().take();
        drop(target);
    }
}
