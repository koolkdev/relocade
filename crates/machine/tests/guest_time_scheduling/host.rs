use super::*;
use futures_channel::mpsc;
use relocade_clock_test_support::ManualClock;
use relocade_machine::{ClockSource, GuestTimeConfig, Sleep};

#[derive(Clone)]
pub(super) struct ObservedHost {
    time: ManualClock,
    armed: mpsc::UnboundedSender<Instant>,
}

impl ClockSource for ObservedHost {
    fn now(&self) -> Instant {
        self.time.clock().now()
    }

    fn sleep_until(&self, deadline: Instant) -> Sleep {
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

pub(super) fn host() -> (ManualClock, mpsc::UnboundedReceiver<Instant>, ObservedHost) {
    let time = ManualClock::new();
    let (armed, requests) = mpsc::unbounded();
    (time.clone(), requests, ObservedHost { time, armed })
}

pub(super) fn config(host: ObservedHost) -> GuestTimeConfig {
    GuestTimeConfig::new(NonZeroU64::new(1000).unwrap()).with_host_clock(Clock::new(host))
}
