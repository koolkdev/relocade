use super::deferred::DeferredCall;
use std::{cell::RefCell, rc::Rc};

/// Requests leave Wasm before the execution owner starts or polls host work.
pub(super) enum PendingCall<T: 'static> {
    Registered(usize),
    Once(Box<dyn DeferredCall<T>>),
}

/// Callback state contains requests, never running service futures.
#[derive(Default)]
pub(super) struct RuntimeState<T: 'static = ()> {
    pub data: Rc<RefCell<T>>,
    pub pending: Option<PendingCall<T>>,
}

impl<T> RuntimeState<T> {
    pub fn new(data: Rc<RefCell<T>>) -> Self {
        Self {
            data,
            pending: None,
        }
    }
}
