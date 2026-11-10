//! Worker tasks and channels with retained Rust data and posted engine resources.
//!
//! [`Pool`] schedules synchronous jobs on caller-supplied workers. Its sendable
//! [`PoolHandle`] implements `relocade_compiler::CompileExecutor` when the
//! `compiler` feature is enabled. Browser builds require threaded Wasm; use
//! the configuration in `tools/web-build.mjs`.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("workers require a threaded browser build; see tests/browser/README.md");

pub use relocade_derive::WorkerResource;
#[cfg(target_arch = "wasm32")]
pub use web_thread::{AsJs, PostExt};
pub use web_thread::{Error, Post, Result, SendTask as Task};

mod cancellable;
mod channel;
mod executor;
mod pool;
mod resource;
mod wake;
mod worker;
pub use cancellable::CancellableWorker;
pub use channel::{channel, Receiver, ResourceError, Sender};
pub use executor::Executor;
pub use pool::{Pool, PoolError, PoolHandle};
pub use resource::{ResourceTransfer, WorkerResource};
pub use wake::{
    wake_connection, WakeBinding, WakeConnection, WakeEndpoint, WakeHost, WakePeer, WakeTarget,
};
pub use worker::Worker;
