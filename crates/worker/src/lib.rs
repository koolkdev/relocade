//! Dedicated workers with explicit readiness, messages and graceful shutdown.
//!
//! Native entries run on an OS thread; browser entries load a private Wasm
//! instance. Entries own their processing loop and call `ready` after setup.
//! Closing input lets that loop drain accepted messages and return. Neither
//! shutdown nor dropping the owner interrupts running work. Completion futures
//! can outlive the owner; dropping a waiter does not stop the worker.
//!
//! Raw message queues are unbounded. [`RequestWorker`] adds bounded, sequential
//! request/reply processing with independently owned result futures. Consumers
//! choose their request types, handlers and limits.
#![forbid(unsafe_code)]

extern crate self as relocade_worker;

mod message;
mod requests;
pub use message::Message;
/// Composes field encodings for structs and enums. Native messages retain the
/// blanket Send implementation. Browser enum tags follow declaration order;
/// they are a same-build worker protocol, not a persistent storage format.
///
/// ```
/// #[derive(relocade_worker::Message)]
/// struct Reply<M> {
///     module: M,
///     metadata: Vec<u8>,
/// }
/// ```
///
/// Unions have no active-variant information to encode:
///
/// ```compile_fail
/// #[derive(relocade_worker::Message)]
/// union Untagged { word: u32, bytes: [u8; 4] }
/// ```
pub use relocade_worker_derive::Message;
pub use requests::{RequestEndpoint, RequestWorker};

#[cfg(not(target_arch = "wasm32"))]
pub mod native;
#[cfg(target_arch = "wasm32")]
pub mod web;

#[derive(Clone, Debug, Eq, PartialEq, Message)]
pub enum Error {
    Closed,
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Closed => f.write_str("worker channel is closed"),
            Self::Failed(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
