//! Interruptible waits for the timer driver, with a retained wake notification.

use std::{
    sync::Arc,
    task::{Wake, Waker},
    thread,
};
use web_time::Instant;

#[cfg(windows)]
mod windows;

pub(super) struct Wait {
    waker: Waker,
    #[cfg(windows)]
    windows: Option<windows::Wait>,
}

impl Wait {
    pub fn new() -> Self {
        #[cfg(windows)]
        if let Some(wait) = windows::Wait::new() {
            return Self {
                waker: wait.waker(),
                windows: Some(wait),
            };
        }
        Self {
            waker: Waker::from(Arc::new(ThreadWake(thread::current()))),
            #[cfg(windows)]
            windows: None,
        }
    }

    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    pub fn until(&self, deadline: Option<Instant>) {
        #[cfg(windows)]
        if let Some(wait) = &self.windows {
            wait.until(deadline);
            return;
        }
        match deadline {
            Some(deadline) => {
                thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
            }
            None => thread::park(),
        }
    }
}

struct ThreadWake(thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
