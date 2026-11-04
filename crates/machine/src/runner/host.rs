use super::Runner;
use crate::Clock;
use futures_util::future::{select, Either};
use std::{future::Future, pin::pin, time::Duration};

impl<C: Clock> Runner<C> {
    /// Receives an independent host resource while the session keeps executing.
    /// Only use this after the producer reports that the resource has been posted.
    /// While delivery is pending, shared-clock wakes request a browser turn about
    /// once per millisecond. Guest and device work resume after every turn even
    /// when the resource has not arrived. Completion or cancellation stops retries.
    ///
    /// Poll inside this runner's session. An ordinary input wait needs no wrapper.
    /// Timer scheduling and guest checkpoints provide no wall-clock latency bound.
    pub async fn wait_for_host<F: Future>(&self, future: F) -> F::Output {
        let mut future = pin!(future);
        loop {
            let interval = pin!(self.clock.sleep(Duration::from_millis(1)));
            match select(future.as_mut(), interval).await {
                Either::Left((value, _)) => return value,
                Either::Right(_) => self.executor.yield_to_host().await,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{worker::wake_connection, CpuStorage};
    use futures_channel::oneshot;
    use std::{
        cell::RefCell,
        future::{pending, ready},
        task::{Context, Poll, Waker},
    };

    #[derive(Default)]
    struct TestClock(RefCell<Vec<(Duration, oneshot::Sender<()>)>>);

    impl Clock for TestClock {
        fn now(&self) -> Duration {
            Duration::ZERO
        }

        async fn wait_until(&self, deadline: Duration) {
            let (elapsed, wait) = oneshot::channel();
            self.0.borrow_mut().push((deadline, elapsed));
            wait.await.unwrap();
        }
    }

    #[cfg_attr(not(target_arch = "wasm32"), test)]
    #[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
    fn delivery_waits_use_one_millisecond_and_release_cancelled_timers() {
        let storage = CpuStorage::new().unwrap();
        let (_host, endpoint) = wake_connection(storage.interrupt_handle());
        let runner = Runner::new(TestClock::default(), storage, endpoint);
        let mut cx = Context::from_waker(Waker::noop());
        let mut immediate = pin!(runner.wait_for_host(ready(42)));
        assert_eq!(immediate.as_mut().poll(&mut cx), Poll::Ready(42));
        assert!(runner.clock.0.borrow().is_empty());

        let (send, received) = oneshot::channel();
        let mut complete = pin!(runner.wait_for_host(received));
        let mut cancelled = Box::pin(runner.wait_for_host(pending::<()>()));
        assert!(complete.as_mut().poll(&mut cx).is_pending());
        assert!(cancelled.as_mut().poll(&mut cx).is_pending());
        drop(cancelled);
        {
            let timers = runner.clock.0.borrow();
            assert_eq!(timers.len(), 2);
            assert!(timers
                .iter()
                .all(|(deadline, _)| *deadline == Duration::from_millis(1)));
            assert!(!timers[0].1.is_canceled());
            assert!(timers[1].1.is_canceled());
        }
        send.send(73).unwrap();
        assert_eq!(complete.as_mut().poll(&mut cx), Poll::Ready(Ok(73)));
        assert!(runner
            .clock
            .0
            .borrow()
            .iter()
            .all(|(_, timer)| timer.is_canceled()));
    }
}
