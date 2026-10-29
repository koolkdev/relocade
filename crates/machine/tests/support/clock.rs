use relocade_machine::Clock;
use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    rc::Rc,
    task::{Poll, Waker},
    time::Duration,
};

#[derive(Clone, Default)]
pub struct ManualClock(Rc<State>);

#[derive(Default)]
struct State {
    now: Cell<Duration>,
    waiters: RefCell<Vec<Waker>>,
}

impl ManualClock {
    pub fn advance(&self, now: Duration) {
        assert!(now >= self.0.now.get());
        self.0.now.set(now);
        for waker in self.0.waiters.take() {
            waker.wake();
        }
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Duration {
        self.0.now.get()
    }

    async fn wait_until(&self, deadline: Duration) {
        poll_fn(|cx| {
            if self.now() >= deadline {
                return Poll::Ready(());
            }
            let mut waiters = self.0.waiters.borrow_mut();
            if !waiters.iter().any(|waker| waker.will_wake(cx.waker())) {
                waiters.push(cx.waker().clone());
            }
            Poll::Pending
        })
        .await;
    }
}
