use relocade_machine::Clock;
use std::{
    future::poll_fn,
    sync::{Arc, Mutex},
    task::{Poll, Waker},
    time::Duration,
};

#[derive(Clone, Default)]
pub struct ManualClock(Arc<Mutex<State>>);

#[derive(Default)]
struct State {
    now: Duration,
    waiters: Vec<Waker>,
}

impl ManualClock {
    pub fn advance(&self, now: Duration) {
        let waiters = {
            let mut state = self.0.lock().unwrap();
            assert!(now >= state.now);
            state.now = now;
            std::mem::take(&mut state.waiters)
        };
        for waker in waiters {
            waker.wake();
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.lock().unwrap().now
    }

    async fn wait_until(&self, deadline: Duration) {
        poll_fn(|cx| {
            let mut state = self.0.lock().unwrap();
            if state.now >= deadline {
                return Poll::Ready(());
            }
            if !state
                .waiters
                .iter()
                .any(|waker| waker.will_wake(cx.waker()))
            {
                state.waiters.push(cx.waker().clone());
            }
            Poll::Pending
        })
        .await;
    }
}
