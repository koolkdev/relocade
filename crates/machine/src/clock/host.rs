use super::{driver, Clock, ClockSource, HostSync, HostTimerDriver, Instant, Sleep};
use futures_channel::{mpsc, oneshot};

/// Transferable access to the host timer service. Construct a local [`Clock`]
/// after moving the handle to its execution owner. Clones share the host timebase.
#[derive(Clone)]
pub struct TimerHandle {
    requests: mpsc::UnboundedSender<driver::Request>,
}

/// Creates the host timer transport and its worker driver.
/// Start [`HostTimerDriver::run`] or [`HostTimerDriver::spawn`] alongside execution.
/// Dropping a sleep cancels its timer; releasing all handles and clocks lets the
/// driver finish after its pending sleeps complete or are cancelled.
pub fn timer_service() -> (TimerHandle, HostTimerDriver) {
    let (requests, receiver) = mpsc::unbounded();
    (TimerHandle { requests }, HostTimerDriver::new(receiver))
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
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sync_to_host(&self, time: Instant) -> HostSync {
        HostSync::new(self.clone(), time)
    }

    fn sleep_until(&self, deadline: Instant) -> Sleep {
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
