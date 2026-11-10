use super::{driver, Clock, ClockSource, HostTimerDriver, Instant, Sleep, Timebase, Timestamp};
use futures_channel::{mpsc, oneshot};

/// Transferable access to the host timer service. Construct a local [`Clock`]
/// after moving the handle to its execution owner. Clones share the host timebase.
#[derive(Clone)]
pub struct TimerHandle {
    origin: Instant,
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
            origin: Instant::now(),
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

impl ClockSource for TimerHandle {
    fn now(&self) -> Timestamp {
        self.host_timestamp(Instant::now()).unwrap()
    }

    fn host_timestamp(&self, time: Instant) -> Option<Timestamp> {
        Some(Timestamp(
            time.saturating_duration_since(self.origin).as_nanos(),
        ))
    }

    fn sync_to_host(&self, time: Timestamp) -> Sleep {
        self.clock().sleep_until_if_needed(time)
    }

    fn sleep_until(&self, deadline: Timestamp) -> Sleep {
        let deadline = self.origin + Timebase::NANOSECONDS.duration_since(deadline, Timestamp(0));
        let handle = self.clone();
        Sleep::new(async move {
            let (complete, completed) = oneshot::channel();
            handle
                .requests
                .unbounded_send(driver::Request { deadline, complete })
                .expect("timer service must outlive its clocks");
            completed
                .await
                .expect("timer service must remain active while a sleep is pending");
        })
    }
}
