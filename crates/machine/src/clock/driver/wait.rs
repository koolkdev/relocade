//! Interruptible waits for the timer driver, with a retained wake notification.

use std::{
    sync::Arc,
    task::{Wake, Waker},
    thread,
};
use web_time::Instant;

#[cfg(target_arch = "wasm32")]
mod browser;
#[cfg(windows)]
mod windows;

pub(super) struct Wait {
    waker: Waker,
    backend: Backend,
}

enum Backend {
    Park,
    #[cfg(windows)]
    Windows(windows::Wait),
    #[cfg(target_arch = "wasm32")]
    Browser(browser::Wait),
}

impl Wait {
    pub fn new() -> Self {
        #[cfg(windows)]
        if let Some(wait) = windows::Wait::new() {
            return Self {
                waker: wait.waker(),
                backend: Backend::Windows(wait),
            };
        }
        #[cfg(target_arch = "wasm32")]
        if browser::environment::needs_message_timer() {
            let wait = browser::Wait::new();
            return Self {
                waker: wait.waker(),
                backend: Backend::Browser(wait),
            };
        }
        Self {
            waker: Waker::from(Arc::new(ThreadWake(thread::current()))),
            backend: Backend::Park,
        }
    }

    pub fn waker(&self) -> &Waker {
        &self.waker
    }

    pub async fn until(&mut self, deadline: Option<Instant>) {
        match &mut self.backend {
            #[cfg(windows)]
            Backend::Windows(wait) => wait.until(deadline),
            #[cfg(target_arch = "wasm32")]
            Backend::Browser(wait) => wait.until(deadline).await,
            Backend::Park => match deadline {
                Some(deadline) => {
                    thread::park_timeout(deadline.saturating_duration_since(Instant::now()));
                }
                None => thread::park(),
            },
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
