use futures_channel::{mpsc, oneshot};
use futures_util::{
    future::{select, Either},
    StreamExt,
};
use std::pin::pin;
use web_time::Instant;

pub(super) struct Request {
    pub deadline: Instant,
    pub complete: oneshot::Sender<()>,
}

/// Owns host timers and delivers their completion to clock waiters.
/// This future can run on the host event loop without creating another thread.
pub struct TimerDriver {
    requests: mpsc::UnboundedReceiver<Request>,
}

impl TimerDriver {
    pub(super) fn new(requests: mpsc::UnboundedReceiver<Request>) -> Self {
        Self { requests }
    }

    /// Services waits until every timer handle and pending wait is released.
    pub async fn run(self) {
        self.requests.for_each_concurrent(None, Request::wait).await;
    }
}

impl Request {
    async fn wait(mut self) {
        let sleep = async {
            loop {
                let remaining = self.deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return;
                }
                futures_timer::Delay::new(remaining).await;
            }
        };
        let elapsed = {
            let cancelled = self.complete.cancellation();
            matches!(select(pin!(cancelled), pin!(sleep)).await, Either::Right(_))
        };
        if elapsed {
            let _ = self.complete.send(());
        }
    }
}
