//! Worker tasks with retained Rust data and posted engine resources.
//!
//! [`Pool`] schedules synchronous jobs on caller-supplied workers. Its sendable
//! [`PoolHandle`] implements `relocade_compiler::CompileExecutor` when the
//! `compiler` feature is enabled. Browser builds require threaded Wasm; use
//! the configuration in `tools/web-build.mjs`.
#![deny(unsafe_code)]

#[cfg(all(target_arch = "wasm32", not(target_feature = "atomics")))]
compile_error!("workers require a threaded browser build; see tests/browser/README.md");

pub use relocade_derive::WorkerResource;
pub use web_thread::*;

use std::future::Future;

mod cancellable;
mod channel;
mod executor;
pub mod oneshot;
mod pool;
mod resource;
mod wake;
pub use cancellable::CancellableWorker;
pub use channel::{channel, Receiver, ReceiverEndpoint, ResourceError, Sender};
pub use executor::Executor;
pub use pool::{Pool, PoolError, PoolHandle};
pub use resource::{ResourceTransfer, WorkerResource};
pub use wake::{
    wake_connection, WakeBinding, WakeConnection, WakeEndpoint, WakeHost, WakePeer, WakeTarget,
};

/// Typed resource tasks on the existing [`Thread`], with the same task lifetime.
pub trait ThreadExt {
    fn run_resource<R, F>(
        &self,
        resource: R,
        code: impl FnOnce(R) -> F + Send + 'static,
    ) -> SendTask<F::Output>
    where
        R: WorkerResource,
        F: Future + 'static,
        F::Output: Send;
}

impl ThreadExt for Thread {
    fn run_resource<R, F>(
        &self,
        resource: R,
        code: impl FnOnce(R) -> F + Send + 'static,
    ) -> SendTask<F::Output>
    where
        R: WorkerResource,
        F: Future + 'static,
        F::Output: Send,
    {
        let ResourceTransfer { posted, rebuild } = resource.prepare();
        self.run_send(posted, move |posted| code(rebuild(posted)))
    }
}
