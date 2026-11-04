//! Requests to return through the host event loop once, then resume Rust work.

use futures_channel::oneshot;
use std::cell::RefCell;

#[derive(Default)]
pub(super) struct HostTurns {
    waiting: RefCell<Vec<oneshot::Sender<()>>>,
}

impl HostTurns {
    pub(super) fn request(&self) -> oneshot::Receiver<()> {
        let (resume, resumed) = oneshot::channel();
        self.waiting.borrow_mut().push(resume);
        resumed
    }

    pub(super) fn has_waiters(&self) -> bool {
        self.waiting
            .borrow()
            .iter()
            .any(|waiter| !waiter.is_canceled())
    }

    pub(super) fn resume(&self) {
        // A child waker can reenter the owner. Release the borrow before sending.
        let waiting = std::mem::take(&mut *self.waiting.borrow_mut());
        for waiter in waiting {
            let _ = waiter.send(());
        }
    }
}
