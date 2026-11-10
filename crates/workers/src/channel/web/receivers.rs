//! Browser objects stay on their owner; tasks carry only a sendable receiver token.

use super::{Inbox, ResourceError};
use futures_channel::oneshot;
use std::{
    any::Any,
    cell::RefCell,
    collections::HashMap,
    marker::PhantomData,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll},
    thread::{self, ThreadId},
};

thread_local! {
    static INBOXES: RefCell<HashMap<usize, Rc<dyn Any>>> = RefCell::default();
}

pub struct Receiver<P> {
    id: usize,
    owner: ThreadId,
    close: Option<oneshot::Sender<()>>,
    marker: PhantomData<fn() -> P>,
}

impl<P: 'static> Receiver<P> {
    pub(super) fn register(inbox: Inbox<P>) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let id = NEXT
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("worker channel IDs exhausted");
        INBOXES.with_borrow_mut(|inboxes| inboxes.insert(id, Rc::new(RefCell::new(inbox))));
        let (close, closed) = oneshot::channel();
        wasm_bindgen_futures::spawn_local(async move {
            let _ = closed.await;
            remove(id);
        });
        Self {
            id,
            owner: thread::current().id(),
            close: Some(close),
            marker: PhantomData,
        }
    }

    pub fn assert_owner(&self) {
        assert_eq!(
            self.owner,
            thread::current().id(),
            "resource receiver used outside its destination worker"
        );
    }

    pub fn poll_recv(&mut self, cx: &mut Context<'_>) -> Poll<Result<P, ResourceError>> {
        // Replacing a queued waker may drop a task holding another receiver.
        // Release the registry borrow before polling as well as before destruction.
        let inbox = INBOXES
            .with_borrow(|inboxes| inboxes.get(&self.id).expect("registered receiver").clone());
        let result = inbox
            .downcast_ref::<RefCell<Inbox<P>>>()
            .expect("receiver payload type")
            .borrow_mut()
            .poll_recv(cx);
        result
    }
}

fn remove(id: usize) {
    // Dropping queued objects can release other receivers. Do it outside the borrow.
    let inbox = INBOXES.with_borrow_mut(|inboxes| inboxes.remove(&id));
    drop(inbox);
}

impl<P> Drop for Receiver<P> {
    fn drop(&mut self) {
        if self.owner == thread::current().id() {
            remove(self.id);
        }
        // Off-owner drops (including cancelled setup) release local objects on
        // the destination's next executor turn.
        drop(self.close.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_channel::mpsc;
    use wasm_bindgen::closure::Closure;
    use web_sys::{MessageChannel, MessageEvent};

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn replacing_a_waiter_can_drop_another_receiver() {
        use std::sync::{Arc, Mutex};
        use std::task::{Wake, Waker};

        struct WaitingTask {
            _receiver: Mutex<Receiver<u32>>,
        }
        #[allow(
            clippy::manual_noop_waker,
            reason = "replacing this waker must drop its owned receiver to exercise nested cleanup"
        )]
        impl Wake for WaitingTask {
            fn wake(self: Arc<Self>) {}
        }

        let (_, endpoint) = super::super::channel::<u32>().unwrap();
        let mut receiver = endpoint.bind();
        let (_, nested) = super::super::channel::<u32>().unwrap();
        let nested = nested.bind();
        let nested_id = nested.id;
        let waker = Waker::from(Arc::new(WaitingTask {
            _receiver: Mutex::new(nested),
        }));
        assert!(receiver
            .poll_recv(&mut Context::from_waker(&waker))
            .is_pending());
        drop(waker);
        assert!(receiver
            .poll_recv(&mut Context::from_waker(Waker::noop()))
            .is_pending());
        INBOXES.with_borrow(|inboxes| assert!(!inboxes.contains_key(&nested_id)));
    }

    struct Released {
        owner: ThreadId,
        done: Option<oneshot::Sender<()>>,
        _local: std::rc::Rc<()>,
        _nested: Receiver<u32>,
    }

    impl Drop for Released {
        fn drop(&mut self) {
            assert_eq!(thread::current().id(), self.owner);
            let _ = self.done.take().unwrap().send(());
        }
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn dropping_a_token_off_owner_releases_local_objects() {
        let worker = crate::Worker::new();
        let (done, released) = oneshot::channel();
        let receiver = worker
            .run(move || async move {
                let (_, nested) = super::super::channel::<u32>().unwrap();
                let (sender, incoming) = mpsc::unbounded();
                sender
                    .unbounded_send(Ok(Released {
                        owner: thread::current().id(),
                        done: Some(done),
                        _local: std::rc::Rc::new(()),
                        _nested: nested.bind(),
                    }))
                    .ok()
                    .unwrap();
                Receiver::register(Inbox {
                    port: MessageChannel::new().unwrap().port1(),
                    incoming,
                    _listener: Closure::<dyn FnMut(MessageEvent)>::new(|_| {}),
                    _errors: Closure::<dyn FnMut(MessageEvent)>::new(|_| {}),
                })
            })
            .await
            .unwrap();
        drop(receiver);
        released.await.unwrap();
        // Cleanup also drops the nested receiver, which borrows the registry again.
        worker
            .run(|| async {
                INBOXES.with_borrow(|inboxes| assert!(inboxes.is_empty()));
            })
            .await
            .unwrap();
    }
}
