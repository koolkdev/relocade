use crate::HostCall;
use std::{
    future::Future,
    ops::{ControlFlow, Deref, DerefMut},
    pin::Pin,
};

/// Backend-owned state for one runtime, including a suspended host operation.
/// Pass this to the backend's `with_state` constructor. Runtime and host-call
/// accessors lend `data` directly. Neither the data nor pending work needs Send.
#[derive(Default)]
pub struct RuntimeState<T = ()> {
    /// Environment data lent by `Runtime::state` and `HostCall::state`.
    pub data: T,
    pub(crate) pending: Option<PendingCall<T>>,
}

impl<T> RuntimeState<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            pending: None,
        }
    }
}

pub(crate) type Completion<T> = Box<dyn FnOnce(&mut HostCall<'_, T>) -> ControlFlow<()>>;

pub(crate) type PendingCall<T> = Pin<Box<dyn Future<Output = Completion<T>>>>;

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
