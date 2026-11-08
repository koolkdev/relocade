use super::{driver, Clock, HostTimerDriver, Sleep, Source, Timestamp};
use futures_channel::{mpsc, oneshot};
use web_time::Instant;

/// Transferable access to the host timer service. Construct a local [`Clock`]
/// after moving the handle to its execution owner. Clones preserve the origin.
#[derive(Clone)]
pub struct TimerHandle {
    start: Instant,
    requests: mpsc::UnboundedSender<driver::Request>,
}

/// Creates the host timer transport and its worker driver.
/// Start [`HostTimerDriver::run`] or [`HostTimerDriver::spawn`] alongside execution.
/// Dropping a sleep cancels its timer; releasing all handles and clocks lets the
/// driver finish after its pending sleeps complete or are cancelled.
pub fn timer_service() -> (TimerHandle, HostTimerDriver) {
    let (requests, receiver) = mpsc::unbounded();
    (
        TimerHandle {
            start: Instant::now(),
            requests,
        },
        HostTimerDriver::new(receiver),
    )
}

impl TimerHandle {
    pub fn clock(&self) -> Clock {
        Clock::new(self.clone())
    }
}

impl From<TimerHandle> for Clock {
    fn from(handle: TimerHandle) -> Self {
        Self::new(handle)
    }
}

impl Source for TimerHandle {
    fn now(&self) -> Timestamp {
        Timestamp::ZERO + self.start.elapsed()
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        let handle = self.clone();
        Sleep::new(async move {
            let (complete, completed) = oneshot::channel();
            handle
                .requests
                .unbounded_send(driver::Request {
                    deadline: handle.start + deadline.duration_since(Timestamp::ZERO),
                    complete,
                })
                .expect("timer service must outlive its clocks");
            completed
                .await
                .expect("timer service must remain active while a sleep is pending");
        })
    }
}
