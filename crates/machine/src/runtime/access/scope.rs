//! Unique invocation capabilities over storage lent by the execution driver.
#![allow(unsafe_code)]

use crate::CpuAccess;
use relocade_wasm_runtime::MemoryAccess;
use std::{cell::Cell, marker::PhantomData, num::NonZeroU64, ptr::NonNull, rc::Rc};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct InvocationId(NonZeroU64);

pub(crate) struct Scope<T> {
    next: Cell<u64>,
    active: Cell<Option<InvocationId>>,
    memory: Cell<NonNull<()>>,
    state: Cell<NonNull<T>>,
    // Keep the environment invariant and every capability on its own thread.
    marker: PhantomData<Rc<Cell<T>>>,
}

pub(super) struct Invocation<T> {
    pub id: InvocationId,
    pub cpu: InvocationAccess<T>,
    pub state: EnvironmentAccess<T>,
}

/// Unique access to an invocation's CPU, valid only during its future poll.
/// Moving this handle is allowed; cloning it is not. Its Rust borrows exclude
/// overlapping mutable access without a runtime borrow guard.
///
/// ```compile_fail
/// # use relocade_machine::InvocationAccess;
/// fn duplicate(access: InvocationAccess) { let _ = access.clone(); }
/// ```
pub struct InvocationAccess<T = ()> {
    scope: Rc<Scope<T>>,
    invocation: InvocationId,
}

/// Scoped access to an invocation's environment, independent of CPU access.
/// Capture owned service resources before awaiting. References returned to a
/// callback cannot escape it or survive a pending poll.
///
/// ```compile_fail
/// # use relocade_machine::EnvironmentAccess;
/// fn duplicate(access: EnvironmentAccess<u32>) { let _ = access.clone(); }
/// ```
///
/// The environment type cannot be changed to accept shorter-lived references:
/// ```compile_fail
/// # use relocade_machine::EnvironmentAccess;
/// fn shorten<'a>(access: EnvironmentAccess<&'static u8>, _: &'a u8)
///     -> EnvironmentAccess<&'a u8> { access }
/// ```
///
/// Shared references are also confined to their callback:
/// ```compile_fail
/// # use relocade_machine::EnvironmentAccess;
/// fn escape(access: &EnvironmentAccess<u32>) -> &u32 {
///     access.with(|state| state)
/// }
/// ```
pub struct EnvironmentAccess<T> {
    scope: Rc<Scope<T>>,
    invocation: InvocationId,
}

impl<T> Scope<T> {
    pub fn new() -> Self {
        Self {
            next: Cell::new(0),
            active: Cell::new(None),
            memory: Cell::new(NonNull::dangling()),
            state: Cell::new(NonNull::dangling()),
            marker: PhantomData,
        }
    }

    pub(super) fn invocation(self: &Rc<Self>) -> Invocation<T> {
        let next = self
            .next
            .get()
            .checked_add(1)
            .expect("invocation IDs exhausted");
        self.next.set(next);
        let id = InvocationId(NonZeroU64::new(next).expect("positive invocation ID"));
        // This is the only constructor: each ID has exactly one capability for
        // CPU storage and one for the disjoint environment. Neither is Clone.
        Invocation {
            id,
            cpu: InvocationAccess {
                scope: self.clone(),
                invocation: id,
            },
            state: EnvironmentAccess {
                scope: self.clone(),
                invocation: id,
            },
        }
    }

    #[inline]
    fn validate(&self, id: InvocationId) {
        assert_eq!(
            self.active.get(),
            Some(id),
            "invocation access is not bound"
        );
    }

    pub fn bind<R>(
        &self,
        id: InvocationId,
        mut memory: MemoryAccess<'_>,
        state: &mut T,
        operation: impl FnOnce() -> R,
    ) -> R {
        assert!(self.active.get().is_none(), "invocation already bound");
        self.memory.set(NonNull::from(&mut memory).cast());
        self.state.set(NonNull::from(state));
        self.active.set(Some(id));
        // Private cleanup cannot be retained or forgotten by the callback, and
        // clears access before the borrowed storage is destroyed, even on unwind.
        let _reset = Reset(self);
        operation()
    }
}

impl<T> CpuAccess for InvocationAccess<T> {
    #[inline]
    fn with<R>(&self, operation: impl FnOnce(&MemoryAccess<'_>) -> R) -> R {
        self.scope.validate(self.invocation);
        // SAFETY: validation establishes the live binding. This unique CPU
        // capability is shared-borrowed, so only other shared CPU reads can
        // coexist. Environment access uses a disjoint pointer. The callback
        // cannot retain the reference beyond this operation.
        let memory = unsafe { &*self.scope.memory.get().cast::<MemoryAccess<'_>>().as_ptr() };
        operation(memory)
    }

    #[inline]
    fn with_mut<R>(&mut self, operation: impl FnOnce(&mut MemoryAccess<'_>) -> R) -> R {
        self.scope.validate(self.invocation);
        // SAFETY: the unique capability's exclusive borrow excludes all other
        // CPU accesses. Reborrow the stored access into a fresh value so the
        // callback cannot replace the binding with shorter-lived storage.
        let memory = unsafe { &mut *self.scope.memory.get().cast::<MemoryAccess<'_>>().as_ptr() };
        operation(&mut memory.reborrow())
    }
}

impl<T> EnvironmentAccess<T> {
    /// Reads environment state synchronously; return owned values for async work.
    #[inline]
    pub fn with<R>(&self, operation: impl FnOnce(&T) -> R) -> R {
        self.scope.validate(self.invocation);
        // SAFETY: only this unique environment capability can access this
        // pointer. Its shared borrow excludes mutation; CPU storage is disjoint.
        operation(unsafe { self.scope.state.get().as_ref() })
    }

    /// Mutates environment state synchronously, without borrowing CPU storage.
    #[inline]
    pub fn with_mut<R>(&mut self, operation: impl FnOnce(&mut T) -> R) -> R {
        self.scope.validate(self.invocation);
        // SAFETY: this exclusive borrow of the unique environment capability
        // excludes other environment accesses, including through panic hooks.
        operation(unsafe { self.scope.state.get().as_mut() })
    }
}

struct Reset<'a, T>(&'a Scope<T>);

impl<T> Drop for Reset<'_, T> {
    fn drop(&mut self) {
        self.0.active.set(None);
        self.0.memory.set(NonNull::dangling());
        self.0.state.set(NonNull::dangling());
    }
}
