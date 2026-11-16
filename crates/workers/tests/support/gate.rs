use futures_channel::oneshot;
use std::{
    future::Future,
    pin::pin,
    sync::Arc,
    task::{Context, Wake, Waker},
    thread,
};

/// Holds a synchronous worker job until the sender is dropped or sends a value.
/// Only the waiting worker parks; releasing it never blocks the browser main thread.
pub fn blocking_gate() -> (oneshot::Sender<()>, impl FnOnce() + Send) {
    let (release, released) = oneshot::channel();
    (release, move || {
        let waker = Waker::from(Arc::new(ThreadWake(thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut released = pin!(released);
        while released.as_mut().poll(&mut cx).is_pending() {
            // Unpark retains a notification arriving between the poll and park.
            thread::park();
        }
    })
}

struct ThreadWake(thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
