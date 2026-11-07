//! Reusable transport for resources that require posting between browser workers.
//!
//! Requests and notifications use ordinary Rust channels. Resource sends queue
//! engine handles; receipt on a browser worker requires an event-loop turn.

use super::{Post, ResourceTransfer, WorkerResource};

#[cfg_attr(not(target_arch = "wasm32"), path = "channel/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "channel/web.rs")]
mod platform;
pub use platform::{channel, Endpoint, Port};

impl<T: Post + 'static> WorkerResource for Endpoint<T> {
    type Posted = Self;

    fn prepare(self) -> ResourceTransfer<Self, impl FnOnce(Self) -> Self + Send + 'static> {
        ResourceTransfer::new(self, std::convert::identity)
    }
}

impl<T: Post + 'static> Port<T> {
    /// Receives the next resource. Dropping this waiter preserves queued values.
    /// `None` follows the bound peer's final message when it closes normally. Browser
    /// worker termination does not promise a close notification or Rust cleanup.
    pub async fn recv(&mut self) -> Option<crate::Result<T>> {
        std::future::poll_fn(|cx| self.poll_recv(cx)).await
    }
}
