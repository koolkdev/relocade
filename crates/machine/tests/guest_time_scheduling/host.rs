use super::*;
use futures_channel::mpsc;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{ClockSource, GuestTimeConfig, Sleep};

#[derive(Clone)]
pub(super) struct ObservedHost {
    time: ManualClock,
    armed: mpsc::UnboundedSender<Timestamp>,
}

impl ClockSource for ObservedHost {
    fn now(&self) -> Timestamp {
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
    (time.clone(), requests, ObservedHost { time, armed })
}

pub(super) fn config() -> GuestTimeConfig {
    let rate = NonZeroU64::new(1000).unwrap();
    GuestTimeConfig::new(rate).with_cpu_limit(rate)
}
