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
mod executor;
mod pool;
mod wake;
pub use cancellable::CancellableWorker;
pub use executor::Executor;
pub use pool::{Pool, PoolError, PoolHandle};
pub use wake::{
    wake_connection, WakeBinding, WakeConnection, WakeEndpoint, WakeHost, WakePeer, WakeTarget,
};

/// A typed handle that can be reconstructed on another worker.
///
/// The posted part uses the existing worker transport. Reconstruction captures
/// retain ordinary `Send` Rust data without serializing it. The reconstructed
/// handle and the future using it need not be `Send`.
///
/// Derive this trait on a struct with one `#[worker(post)]` field. All other
/// fields are retained as `Send + 'static` Rust captures, without serialization.
/// Named and tuple structs use the same transfer mechanism.
/// Set `#[worker(crate = relocade_workers)]` when using this crate directly;
/// the default path supports the `relocade_wasm_runtime::worker` re-export.
///
/// ```
/// use relocade_workers::WorkerResource;
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Configured<T, P> {
///     config: T,
///     #[worker(post)]
///     backing: P,
/// }
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Handle(#[worker(post)] ());
/// # trait Configuration { type Data; }
/// # #[derive(WorkerResource)]
/// # #[worker(crate = relocade_workers)]
/// # struct Associated<C: Configuration> {
/// #     config: C::Data,
/// #     #[worker(post)]
/// #     backing: (),
/// # }
/// ```
///
/// Captured fields must be safe to move to another thread:
///
/// ```compile_fail
/// use relocade_workers::WorkerResource;
///
/// #[derive(WorkerResource)]
/// #[worker(crate = relocade_workers)]
/// struct Local {
///     local: std::rc::Rc<()>,
///     #[worker(post)]
///     backing: (),
/// }
/// ```
pub trait WorkerResource: Sized + 'static {
    type Posted: Post;

    fn prepare(
        self,
    ) -> ResourceTransfer<Self::Posted, impl FnOnce(Self::Posted) -> Self + Send + 'static>;
}

/// A resource and its reconstruction operation, consumed by [`ThreadExt::run_resource`].
/// Private fields keep a capability's underlying storage inaccessible to callers.
pub struct ResourceTransfer<P, R> {
    posted: P,
    rebuild: R,
}

impl<P, R> ResourceTransfer<P, R> {
    /// Prepares a transfer inside the resource owner, where its fields are accessible.
    pub fn new(posted: P, rebuild: R) -> Self {
        Self { posted, rebuild }
    }
}

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
