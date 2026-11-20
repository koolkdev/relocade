use super::*;
use futures_channel::mpsc;
use futures_util::StreamExt;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{ClockSource, GuestTimeConfig, Sleep};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[derive(Clone)]
pub(super) struct ObservedHost {
    time: ManualClock,
    pub(super) reads: Arc<AtomicUsize>,
    armed: mpsc::UnboundedSender<Timestamp>,
}

impl ClockSource for ObservedHost {
    fn now(&self) -> Timestamp {
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.time.clock().now()
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        let time = self.time.clock();
        let mut armed = Some(self.armed.clone());
        Sleep::new(async move {
            let mut sleep = pin!(time.sleep_until(deadline));
            poll_fn(|cx| {
                let result = sleep.as_mut().poll(cx);
                if let Some(armed) = armed.take() {
                    let _ = armed.unbounded_send(deadline);
                }
                result
            })
            .await
        })
    }
}

pub(super) fn host() -> (
    ManualClock,
    mpsc::UnboundedReceiver<Timestamp>,
    ObservedHost,
) {
    let time = ManualClock::new();
    let (armed, requests) = mpsc::unbounded();
    (
        time.clone(),
        requests,
        ObservedHost {
            time,
            armed,
            reads: Default::default(),
        },
    )
}

pub(super) fn config() -> GuestTimeConfig {
    let rate = NonZeroU64::new(1000).unwrap();
    GuestTimeConfig::new(rate).with_cpu_limit(rate)
}

/// The CPU may arm a calibration timer before reaching HLT or its pacing boundary.
/// Advancing only to the expected deadline exercises the parked worker.
pub(super) async fn expect_parked_timer(
    armed: &mut mpsc::UnboundedReceiver<Timestamp>,
    host: &ManualClock,
    expected: Timestamp,
) {
    let clock = host.clock();
    let sample = clock.deadline_after(clock.now(), Duration::from_millis(4));
    while let Some(at) = armed.next().await {
        if at == expected {
            return;
        }
        assert_eq!(at, sample);
    }
    panic!("the worker ended before arming {expected:?}");
}
