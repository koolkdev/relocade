//! Workers and reusable channels for posting engine resources between them.
//!
//! Use ordinary Rust channels for requests and notifications. A resource channel
//! carries values such as compiled modules that cannot move through Rust memory
//! on every backend. Transfer an endpoint as a worker context, then bind it to
//! a local port. Successful sends queue messages; they do not acknowledge receipt.

pub use web_thread::*;

#[cfg_attr(not(target_arch = "wasm32"), path = "worker/native.rs")]
#[cfg_attr(target_arch = "wasm32", path = "worker/web.rs")]
mod channel;
pub use channel::{channel, Endpoint, Port};

impl<T: Post + 'static> Port<T> {
    /// Receives the next resource. Dropping this waiter preserves queued values.
    /// `None` follows the bound peer's final message when it closes normally. Browser
    /// worker termination does not promise a close notification or Rust cleanup.
    pub async fn recv(&mut self) -> Option<crate::Result<T>> {
        std::future::poll_fn(|cx| self.poll_recv(cx)).await
    }
}
