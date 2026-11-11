//! Worker tasks with retained Rust data and posted engine resources.
//!
//! [`Thread`] submits tasks on a reusable execution thread. [`Worker`] owns one
//! task through startup, cooperative shutdown and cleanup. [`Transfer`] defines
//! how each message's fields cross threads.
//!
//! [`Pool`] schedules synchronous jobs on caller-supplied workers. Its sendable
//! [`PoolHandle`] implements `relocade_compiler::CompileExecutor` when the
//! `compiler` feature is enabled. Browser builds require threaded Wasm; use
//! the configuration in `tools/web-build.mjs`.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("workers require a threaded browser build; see tests/browser/README.md");

pub use relocade_derive::Transfer;
#[cfg(target_arch = "wasm32")]
pub use web_thread::{AsJs, PostExt};
pub use web_thread::{Error, Post, Result, SendTask};

#[cfg(not(target_arch = "wasm32"))]
pub use std::thread::available_parallelism;
#[cfg(target_arch = "wasm32")]
#[path = "parallelism/web.rs"]
mod parallelism;
#[cfg(target_arch = "wasm32")]
pub use parallelism::available_parallelism;

mod channel;
mod executor;
mod pool;
mod thread;
mod transfer;
mod wake;
mod worker;
pub use channel::{channel, channel_endpoints, ChannelError, Receiver, ReceiverEndpoint, Sender};
pub use executor::Executor;
pub use pool::{Pool, PoolError, PoolHandle};
pub use thread::Thread;
pub use transfer::Transfer;
#[doc(hidden)]
pub use transfer::{PayloadOption, PayloadPair};
pub use wake::{
    wake_connection, WakeBinding, WakeConnection, WakeEndpoint, WakeHost, WakePeer, WakeTarget,
};
pub use worker::{Worker, WorkerError};
