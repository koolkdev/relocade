//! The lifetime bridge between a retained service and one borrowed CPU poll.
//!
//! A handler's higher-ranked argument cannot escape into its static captures or
//! an independent task. The only retained borrows are of the stable access slot
//! and registered handler. A pending Call owns both and drops its future first.
//! The driver must never poll or destroy another retained call under an active
//! binding. Only the selected invocation can execute; parked futures are private.

use super::hooks::{Handler, Started};
use super::HookResult;
use crate::{CpuAccess, CpuStorage, CpuView};
use futures_util::future::LocalBoxFuture;
use relocade_x86::ExecutionTarget;
use std::{
    cell::Cell,
    ptr::NonNull,
    rc::Rc,
    task::{Context, Poll},
};

/// Unique CPU access belonging to one invocation. It borrows a stable binding
/// slot, not backend storage. Only the execution driver can create this provider.
pub struct ScopedAccess<'call> {
    scope: &'call AccessScope,
}

/// A service's live registers. Ordinary awaits retain this capability while the
/// runner releases backend storage. Each poll binds the current CPU state again.
/// The capability cannot be cloned, exported or polled independently of its call.
pub type HookCpu<'call> = CpuView<ScopedAccess<'call>>;

impl CpuAccess for ScopedAccess<'_> {
    #[inline]
    fn is_bound(&self) -> bool {
        self.scope.storage.get().is_some()
    }

    #[inline]
    fn with<R>(&self, operation: impl FnOnce(&CpuStorage<'_>) -> R) -> R {
        let storage = self
            .scope
            .storage
            .get()
            .expect("CPU access outside a service poll");
        // SAFETY: bind owns the live CpuStorage for this callback. Only this
        // invocation has a capability, and &self excludes its mutable accesses.
        // The callback's result cannot borrow the storage.
        unsafe { operation(storage.cast::<CpuStorage<'_>>().as_ref()) }
    }

    #[inline]
    fn with_mut<R>(&mut self, operation: impl FnOnce(&mut CpuStorage<'_>) -> R) -> R {
        let storage = self
            .scope
            .storage
            .get()
            .expect("CPU access outside a service poll");
        // SAFETY: the unique capability and &mut self exclude overlapping access.
        // Lend a fresh reborrow so replacing the callback's view cannot alter the
        // bound storage or install references with a shorter lifetime in it.
        unsafe { operation(&mut storage.cast::<CpuStorage<'_>>().as_mut().reborrow()) }
    }
}

#[derive(Default)]
pub(super) struct AccessScope {
    storage: Cell<Option<NonNull<()>>>,
}

impl AccessScope {
    fn bind<R>(&self, mut storage: CpuStorage<'_>, operation: impl FnOnce() -> R) -> R {
        assert!(self.storage.get().is_none(), "CPU poll already bound");
        self.storage.set(Some(NonNull::from(&mut storage).cast()));
        struct Reset<'a>(&'a AccessScope);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                self.0.storage.set(None);
            }
        }
        // This guard is private and cannot be forgotten by callback code. Clear
        // the binding before storage dies, including when polling unwinds.
        let _reset = Reset(self);
        operation()
    }
}

pub(super) enum CallPoll {
    Ready(HookResult),
    Pending(Call),
}

pub(super) struct Call {
    // Drop order is part of the lifetime proof. A future can borrow its handler
    // captures and the scope until its destructor finishes, including on unwind.
    future: LocalBoxFuture<'static, HookResult>,
    handler: Rc<dyn Handler>,
    scope: Rc<AccessScope>,
}

impl Call {
    pub fn start(
        handler: &Rc<dyn Handler>,
        scope: &Rc<AccessScope>,
        target: ExecutionTarget,
        storage: CpuStorage<'_>,
        cx: &mut Context<'_>,
    ) -> CallPoll {
        scope.bind(storage, || {
            let cpu = CpuView::new(target, ScopedAccess { scope });
            match handler.start(cpu, cx) {
                Started::Ready(result) => CallPoll::Ready(result),
                Started::Pending(future) => {
                    // SAFETY: Handler::start can borrow only the handler and the
                    // CPU capability's scope. Both have stable Rc allocations,
                    // retained below until after the pinned future is destroyed.
                    // Call never exports the future or capability; only start and
                    // poll execute it, and cancellation is always unbound.
                    let future = unsafe {
                        std::mem::transmute::<
                            LocalBoxFuture<'_, HookResult>,
                            LocalBoxFuture<'static, HookResult>,
                        >(future)
                    };
                    CallPoll::Pending(Self {
                        future,
                        handler: handler.clone(),
                        scope: scope.clone(),
                    })
                }
            }
        })
    }

    pub fn poll(self, storage: CpuStorage<'_>, cx: &mut Context<'_>) -> CallPoll {
        let Self {
            future,
            handler,
            scope,
        } = self;
        let polled = scope.bind(storage, || {
            // Own the future inside the binding so its destructor still has CPU
            // access on completion or unwind, even for custom Future impls.
            let mut future = future;
            match future.as_mut().poll(cx) {
                Poll::Ready(result) => {
                    drop(future);
                    Ok(result)
                }
                Poll::Pending => Err(future),
            }
        });
        match polled {
            Ok(result) => CallPoll::Ready(result),
            Err(future) => CallPoll::Pending(Self {
                future,
                handler,
                scope,
            }),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
