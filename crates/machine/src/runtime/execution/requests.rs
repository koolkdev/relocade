//! Submits guest-call requests and waits for their completion.

use super::GuestCall;
use futures_channel::oneshot;
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
pub(in crate::runtime) struct GuestCalls(Rc<RefCell<Option<Request>>>);

pub(super) struct Request {
    pub(super) target: GuestCall,
    pub(super) returned: oneshot::Sender<()>,
}

impl GuestCalls {
    pub async fn call(&self, target: GuestCall) {
        let (returned, completion) = oneshot::channel();
        self.put(Request { target, returned });
        completion
            .await
            .expect("guest caller retained until return");
    }

    pub(super) fn put(&self, request: Request) {
        let mut pending = self.0.borrow_mut();
        assert!(
            pending
                .as_ref()
                .is_none_or(|call| call.returned.is_canceled()),
            "only one guest call may be requested by a host handler"
        );
        *pending = Some(request);
    }

    pub(super) fn take(&self) -> Option<Request> {
        // Selecting another future can cancel an invocation before the driver
        // sees it. Such a request must not change the guest's CPU or stack.
        self.0
            .borrow_mut()
            .take()
            .filter(|call| !call.returned.is_canceled())
    }
}
