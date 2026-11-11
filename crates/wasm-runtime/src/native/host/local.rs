//! Adapts local captures to Wasmtime's store-independent callback bound.
#![allow(unsafe_code)]

pub(super) struct Callback<F>(F);

impl<F> Callback<F> {
    pub(super) fn new(callback: F) -> Self {
        Self(callback)
    }

    pub(super) fn get(&self) -> &F {
        &self.0
    }
}

// SAFETY: This private wrapper is constructed only by host::wrap and immediately
// moved into Func::wrap on a Store<StoreData<T>>. StoreData is unconditionally
// !Send and !Sync, as are all contexts borrowing it. Wasmtime roots the closure
// in that store until destruction; safe Func calls and imports reject other
// stores. We never expose the HostFunc that could register this closure elsewhere.
// Construction, invocation and destruction therefore stay on the creation thread.
// No wrapped callback or reference to it escapes the owning store's invocation.
unsafe impl<F> Send for Callback<F> {}
unsafe impl<F> Sync for Callback<F> {}
