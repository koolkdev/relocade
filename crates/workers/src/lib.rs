//! Worker tasks with retained Rust data and posted engine resources.
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

use std::future::Future;

mod cancellable;
mod executor;
mod pool;
mod transfer;
mod wake;
pub use cancellable::CancellableWorker;
pub use executor::Executor;
pub use pool::{Pool, PoolError, PoolHandle};
#[doc(hidden)]
pub use transfer::PayloadPair;
pub use transfer::Transfer;
pub use wake::{
    wake_connection, WakeBinding, WakeConnection, WakeEndpoint, WakeHost, WakePeer, WakeTarget,
};

/// Runs transferred values on an existing [`Thread`], with the same task lifetime.
pub trait ThreadExt {
    fn run_resource<R, F>(
        &self,
        resource: R,
        code: impl FnOnce(R) -> F + Send + 'static,
    ) -> SendTask<F::Output>
    where
        R: Transfer,
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
        R: Transfer,
        F: Future + 'static,
        F::Output: Send,
    {
        let (state, payload) = resource.encode();
        self.run_send(payload, move |payload| code(R::decode(state, payload)))
    }
}
