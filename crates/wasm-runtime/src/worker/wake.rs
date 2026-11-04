//! Notification resources bound once, before a worker session starts.

mod peer;
pub use peer::{WakeBinding, WakePeer};

use std::{
    marker::PhantomData,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Waker,
};

#[cfg_attr(not(target_arch = "wasm32"), path = "wake/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "wake/web.rs")]
mod platform;
pub use platform::wake_connection;

/// A retained signal that asks busy execution to return to Rust.
/// Clones, including handles reconstructed on another thread, must address the
/// same signal. Waking and acknowledgement may run concurrently. Neither may
/// invoke another waker. Native targets must also be `Send + Sync`.
pub trait WakeTarget: Clone + 'static {
    fn wake(&self);
    fn acknowledge(&self);
}

/// Keeps the host's notification resource alive through worker completion.
/// Dropping either end disables subsequent notifications from escaped wakers.
pub struct WakeHost<R: WakeTarget> {
    _lifetime: Lifetime,
    _binding: platform::HostBinding<R>,
    peer: WakePeer<R>,
}

/// Sendable connection metadata, captured alongside an existing posted resource.
/// This token does not post or own a browser resource.
pub struct WakeEndpoint<R: WakeTarget> {
    lifetime: Lifetime,
    notification: Waker,
    binding: platform::Endpoint,
    target: PhantomData<fn() -> R>,
}

/// The execution end of a fixed notification connection.
pub struct WakeConnection<R: WakeTarget> {
    _lifetime: Lifetime,
    _binding: platform::Binding,
    target: R,
    notification: Waker,
    endpoint: platform::Endpoint,
}

impl<R: WakeTarget> WakeEndpoint<R> {
    /// Binds the receiving thread's handle to the same signal supplied at
    /// `wake_connection`. Reconstruct that handle through its existing resource
    /// transport before binding; no registration message or acknowledgement is
    /// needed. A different signal requires a new connection.
    pub fn bind(self, target: R) -> WakeConnection<R> {
        let binding = platform::bind(self.binding, target.clone(), self.lifetime.0.clone());
        WakeConnection {
            _lifetime: self.lifetime,
            _binding: binding,
            target,
            notification: self.notification,
            endpoint: self.binding,
        }
    }
}

impl<R: WakeTarget> WakeConnection<R> {
    /// Requests return to Rust from either connected thread. This only marks
    /// the signal; scheduling an async task also requires its executor waker.
    pub fn waker(&self) -> &Waker {
        &self.notification
    }

    /// Clears prior requests. A scheduler must clear before checking runnable
    /// work, or separately track racing wakes so that none can be lost.
    pub fn acknowledge(&self) {
        self.target.acknowledge();
    }
}

struct Lifetime(Arc<AtomicBool>);
impl Drop for Lifetime {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
