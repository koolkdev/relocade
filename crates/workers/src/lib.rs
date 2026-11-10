//! Worker tasks with retained Rust data and posted engine resources.
//!
//! [`Worker`] starts tasks with optional typed inboxes. [`Transfer`] defines how
//! each message's fields cross threads, while [`CancellableWorker`] retains a
//! task through cooperative shutdown and cleanup.
//!
//! [`Pool`] schedules synchronous jobs on caller-supplied workers. Its sendable
//! [`PoolHandle`] implements `relocade_compiler::CompileExecutor` when the
//! `compiler` feature is enabled. Browser builds require threaded Wasm; use
//! the configuration in `tools/web-build.mjs`.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("workers require a threaded browser build; see tests/browser/README.md");

pub use relocade_derive::Transfer;
pub use web_thread::*;

mod cancellable;
mod channel;
mod executor;
mod pool;
mod transfer;
mod wake;
mod worker;
pub use cancellable::{CancellableWorker, WorkerError};
pub use channel::{channel, ChannelError, Receiver, Sender};
pub use executor::Executor;
pub use pool::{Pool, PoolError, PoolHandle};
#[doc(hidden)]
pub use transfer::PayloadPair;
pub use transfer::Transfer;
pub use wake::{
    wake_connection, WakeBinding, WakeConnection, WakeEndpoint, WakeHost, WakePeer, WakeTarget,
};
pub use worker::Worker;
