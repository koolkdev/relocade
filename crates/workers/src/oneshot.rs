//! One resource sent to a session that is already running on a worker.

use crate::{
    host_turn::HostTurns, Executor, ResourceError, Thread, ThreadExt, WakeTarget, WorkerResource,
};
use futures_channel::oneshot;
use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

thread_local! {
    static RECEIVERS: RefCell<HashMap<usize, Box<dyn Any>>> = RefCell::default();
}
static NEXT_RECEIVER: AtomicUsize = AtomicUsize::new(0);

/// Sends one resource through an existing thread's posting mechanism.
pub struct Sender<R> {
    registered: oneshot::Receiver<Registration>,
    marker: PhantomData<fn(R)>,
}

/// Sendable metadata that can be captured by an ordinary worker session closure.
/// Bind it on the receiving worker before waiting for the resource.
pub struct ReceiverEndpoint<R> {
    register: oneshot::Sender<Registration>,
    marker: PhantomData<fn() -> R>,
}

/// A resource receive bound to this worker. Its registration stays on this owner.
pub struct Receiver<R> {
    _slot: Slot,
    posted: oneshot::Receiver<()>,
    incoming: oneshot::Receiver<R>,
    completed: oneshot::Receiver<Result<(), ResourceError>>,
    host: Option<Rc<HostTurns>>,
}

struct Registration {
    id: usize,
    posted: oneshot::Sender<()>,
    completed: oneshot::Sender<Result<(), ResourceError>>,
}

/// Creates a handoff without creating or transferring a browser object.
/// Capture the endpoint in the receiving session; send through its thread later.
pub fn channel<R: WorkerResource>() -> (Sender<R>, ReceiverEndpoint<R>) {
    let (register, registered) = oneshot::channel();
    (
        Sender {
            registered,
            marker: PhantomData,
        },
        ReceiverEndpoint {
            register,
            marker: PhantomData,
        },
    )
}

impl<R: WorkerResource> Sender<R> {
    /// Waits for the endpoint to bind, then posts to that worker. Await completion
    /// before releasing the thread. The resource itself never crosses Rust channels.
    pub async fn send(self, thread: &Thread, resource: R) -> Result<(), ResourceError> {
        let Registration {
            id,
            posted,
            completed,
        } = self.registered.await.map_err(|_| ResourceError::Closed)?;
        let task = thread.run_resource(resource, move |resource| async move {
            let sender = RECEIVERS.with_borrow_mut(|receivers| receivers.remove(&id));
            let sender = sender.ok_or_else(|| {
                ResourceError::Transport("resource receiver is not bound on this worker".into())
            })?;
            let sender = sender
                .downcast::<oneshot::Sender<R>>()
                .unwrap_or_else(|_| unreachable!("a resource handoff has one fixed type"));
            let _ = sender.send(resource);
            Ok(())
        });
        // Posting can wake a busy session before the host callback runs there.
        let _ = posted.send(());
        let result = task
            .await
            .map_err(|error| ResourceError::Transport(error.to_string()))
            .and_then(|result| result);
        let _ = completed.send(result.clone());
        result
    }
}

impl<R: WorkerResource> ReceiverEndpoint<R> {
    pub fn bind(self) -> Result<Receiver<R>, ResourceError> {
        let (sender, incoming) = oneshot::channel::<R>();
        let slot = Slot::new(sender);
        let (posted, posting) = oneshot::channel();
        let (completed, completion) = oneshot::channel();
        self.register
            .send(Registration {
                id: slot.id,
                posted,
                completed,
            })
            .map_err(|_| ResourceError::Closed)?;
        Ok(Receiver {
            _slot: slot,
            posted: posting,
            incoming,
            completed: completion,
            host: None,
        })
    }

    /// Binds the handoff to a busy worker's receipt policy. Posting and task
    /// completion are handled by `receive`, including their required host turns.
    /// Poll receipt inside this executor's root future.
    pub fn bind_on<T: WakeTarget>(
        self,
        executor: &Executor<T>,
    ) -> Result<Receiver<R>, ResourceError> {
        let mut receiver = self.bind()?;
        receiver.host = Some(executor.host_turns.clone());
        Ok(receiver)
    }
}

impl<R> Receiver<R> {
    /// Receives after the sending task has completed. Keeping this wait pending
    /// lets a busy owner give the posting task its final host turn too. Dispatch
    /// failures complete the wait even if the posted callback never ran.
    pub async fn receive(self) -> Result<R, ResourceError> {
        let Self {
            _slot,
            incoming,
            completed,
            posted,
            host,
        } = self;
        posted.await.map_err(|_| ResourceError::Closed)?;
        let receive = async {
            completed.await.map_err(|_| ResourceError::Closed)??;
            incoming.await.map_err(|_| ResourceError::Closed)
        };
        match host {
            Some(host) => host.receive(receive).await,
            None => receive.await,
        }
    }
}

struct Slot {
    id: usize,
    // The guard must remove its registration on the thread that created it.
    _local: PhantomData<Rc<()>>,
}

impl Slot {
    fn new<R: 'static>(sender: oneshot::Sender<R>) -> Self {
        let id = NEXT_RECEIVER
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("resource receiver ID capacity");
        RECEIVERS.with_borrow_mut(|receivers| receivers.insert(id, Box::new(sender)));
        Self {
            id,
            _local: PhantomData,
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        // Removing before dropping allows channel wakes to access the registry.
        let sender = RECEIVERS.with_borrow_mut(|receivers| receivers.remove(&self.id));
        drop(sender);
    }
}
