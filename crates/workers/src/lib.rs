//! Worker tasks with retained Rust data and posted engine resources.
//!
//! [`Thread`] submits tasks on a reusable execution thread. [`Worker`] owns one
//! task through startup, cooperative shutdown and cleanup. [`Transfer`] defines
//! how each message's fields cross threads.
//!
//! [`JobQueue`] runs calculations on its owning thread; [`Pool`] distributes
//! them across caller-supplied workers. Both return a [`JobExecutor`] for
//! submitting owned calculations. Browser builds require threaded Wasm; use the
//! configuration in `tools/web-build.mjs`.
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
mod jobs;
mod pool;
mod thread;
mod transfer;
mod wake;
mod worker;
mod worker_wake;
pub use channel::{channel, channel_endpoints, ChannelError, Receiver, ReceiverEndpoint, Sender};
pub use jobs::{JobExecutor, JobQueue};
pub use pool::{Pool, PoolError};
pub use thread::Thread;
pub use transfer::Transfer;
#[doc(hidden)]
pub use transfer::{PayloadOption, PayloadPair};
pub use wake::{
    wake_connection, WakeBinding, WakeConnection, WakeEndpoint, WakeHost, WakePeer, WakeTarget,
};
pub use worker::{Worker, WorkerError};
pub use worker_wake::{yield_to_host, HostTurns, WorkerWake};
