use crate::callback::DeferredCall;
use std::ops::{Deref, DerefMut};

/// Backend-owned state for one runtime, including a queued async host handler.
/// Pass this to the backend's `with_state` constructor. Cpu and host-call
/// accessors lend `data` directly. Neither the data nor pending work needs Send.
#[derive(Default)]
pub struct RuntimeState<T = ()> {
    /// Environment data lent by `Cpu::state` and `HostCall::state`.
    pub data: T,
    pub(crate) pending: Option<Box<dyn DeferredCall<T>>>,
}

impl<T> RuntimeState<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            pending: None,
        }
    }
}

// Preserve the backend's borrow guard while lending only environment data.
pub(crate) struct Data<S>(pub S);

impl<T, S: Deref<Target = RuntimeState<T>>> Deref for Data<S> {
    type Target = T;

    fn deref(&self) -> &T {
        &self.0.data
    }
}

impl<T, S: DerefMut<Target = RuntimeState<T>>> DerefMut for Data<S> {
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0.data
    }
}
