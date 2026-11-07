//! Shared time and timer notifications from a separately driven host task.

mod driver;
pub use driver::TimerDriver;

use futures_channel::{mpsc, oneshot};
use std::{future::Future, time::Duration};
use web_time::Instant;

/// Elapsed session time and asynchronous waits on that timebase.
/// Devices and CPU services use the same interface; tests can control time manually.
pub trait Clock {
    fn now(&self) -> Duration;
    fn wait_until(&self, deadline: Duration) -> impl Future<Output = ()>;

    fn sleep(&self, duration: Duration) -> impl Future<Output = ()> {
        self.wait_until(self.now().saturating_add(duration))
    }
}

/// A clonable handle to the host timer service.
/// [`Clock::now`] reads elapsed real time; waits arrive through Rust channels.
/// The timer driver must run on a responsive host, outside a continuously busy
/// emulation worker. Clones preserve their time origin, including across workers.
#[derive(Clone)]
pub struct TimerHandle {
    start: Instant,
    requests: mpsc::UnboundedSender<driver::Request>,
}

/// Creates a timer service handle and its host-side driver.
/// Poll [`TimerDriver::run`] on the host while execution owns the timer handles.
/// Dropping a wait cancels its timer; dropping all handles lets the driver finish.
pub fn timer_service() -> (TimerHandle, TimerDriver) {
    let (requests, receiver) = mpsc::unbounded();
    (
        TimerHandle {
            start: Instant::now(),
            requests,
        },
        TimerDriver::new(receiver),
    )
}

impl Clock for TimerHandle {
    fn now(&self) -> Duration {
        self.start.elapsed()
    }

    async fn wait_until(&self, deadline: Duration) {
        let (complete, completed) = oneshot::channel();
        self.requests
            .unbounded_send(driver::Request {
                deadline: self.start + deadline,
                complete,
            })
            .expect("timer driver must outlive its clock handles");
        completed
            .await
            .expect("timer driver must remain active while a wait is pending");
    }
}
