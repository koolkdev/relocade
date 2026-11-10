//! Direct signal access for additional threads on an existing connection.

use super::{platform, WakeConnection, WakeHost, WakeTarget};
use crate::WorkerResource;
use std::sync::{atomic::AtomicBool, Arc};

/// A clonable signal resource to post when setting up another worker.
/// Bind it on that worker before handing it senders that can wake execution.
/// This shares the original connection's lifetime; peer removal does not close it.
#[derive(Clone, WorkerResource)]
#[worker(crate = crate)]
pub struct WakePeer<R: WakeTarget> {
    #[worker(resource)]
    pub(super) target: R,
    pub(super) endpoint: platform::Endpoint,
    pub(super) active: Arc<AtomicBool>,
}

/// Retains this thread's direct notification binding. Duplicate bindings share
/// their local registration; it is removed when the last guard is dropped.
#[must_use = "retain the binding while this thread can notify execution"]
pub struct WakeBinding {
    _binding: platform::Binding,
}

impl<R: WakeTarget> WakePeer<R> {
    pub fn bind(self) -> WakeBinding {
        WakeBinding {
            _binding: platform::bind(self.endpoint, self.target, self.active),
        }
    }
}

impl<R: WakeTarget> WakeHost<R> {
    /// Shares this connection's signal through the existing resource transport.
    pub fn peer(&self) -> WakePeer<R> {
        self.peer.clone()
    }
}

impl<R: WakeTarget> WakeConnection<R> {
    pub fn peer(&self) -> WakePeer<R> {
        WakePeer {
            target: self.target.clone(),
            endpoint: self.endpoint,
            active: self._lifetime.active.clone(),
        }
    }
}
