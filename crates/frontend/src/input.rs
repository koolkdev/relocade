//! Host timestamps travel with input so queueing does not change event time.

use web_time::Instant;

/// One input value and its monotonic host timestamp.
/// The threaded browser build uses a common origin across workers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputEvent<T> {
    pub host_time: Instant,
    pub event: T,
}

impl<T> InputEvent<T> {
    /// Capture host time immediately before enqueueing an event. Controlled
    /// producers can supply an explicit timestamp with a struct literal.
    pub fn new(event: T) -> Self {
        Self {
            host_time: Instant::now(),
            event,
        }
    }
}
