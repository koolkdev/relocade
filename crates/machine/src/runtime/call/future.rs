//! A callback's pinned future, with its captured owner retained until destruction.

use futures_util::future::LocalBoxFuture;
use std::{
    rc::Rc,
    task::{Context, Poll},
};

pub(super) struct OwnerFuture<Owner: ?Sized + 'static, Output: 'static> {
    // The dependent future must be destroyed before its owner, also on unwind.
    future: LocalBoxFuture<'static, Output>,
    _owner: Rc<Owner>,
}

impl<Owner: ?Sized + 'static, Output: 'static> OwnerFuture<Owner, Output> {
    pub fn new(
        owner: Rc<Owner>,
        construct: impl for<'a> FnOnce(&'a Owner) -> LocalBoxFuture<'a, Output>,
    ) -> Self {
        let future = construct(&owner);
        // SAFETY: the factory can borrow only for the supplied owner's lifetime.
        // Rc keeps that allocation stable when this object moves. Both fields
        // remain private, and field drop order destroys the pinned future before
        // releasing this owner. Polling exposes only Output, which is 'static.
        #[allow(unsafe_code)]
        let future = unsafe {
            std::mem::transmute::<LocalBoxFuture<'_, Output>, LocalBoxFuture<'static, Output>>(
                future,
            )
        };
        Self {
            future,
            _owner: owner,
        }
    }

    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<Output> {
        self.future.as_mut().poll(cx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        future::Future,
        marker::PhantomPinned,
        panic::{catch_unwind, AssertUnwindSafe},
        pin::Pin,
        task::Waker,
    };

    type Events = Rc<RefCell<Vec<&'static str>>>;

    struct Owner {
        text: String,
        events: Events,
    }

    impl Drop for Owner {
        fn drop(&mut self) {
            self.events.borrow_mut().push("owner");
        }
    }

    struct Borrowed<'a> {
        owner: &'a Owner,
        address: Cell<*const Self>,
        panic_poll: bool,
        panic_drop: bool,
        _pin: PhantomPinned,
    }

    impl Future for Borrowed<'_> {
        type Output = usize;

        fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<usize> {
            let this = self.as_ref().get_ref();
            assert_eq!(this.owner.text, "retained callback");
            assert!(!this.panic_poll, "poll panic");
            if this.address.get().is_null() {
                this.address.set(this);
                Poll::Pending
            } else {
                assert_eq!(this.address.get(), this as *const Self);
                Poll::Ready(this.owner.text.len())
            }
        }
    }

    impl Drop for Borrowed<'_> {
        fn drop(&mut self) {
            assert_eq!(self.owner.text, "retained callback");
            self.owner.events.borrow_mut().push("future");
            assert!(!self.panic_drop, "destructor panic");
        }
    }

    fn owner() -> (Rc<Owner>, Events) {
        let events = Rc::new(RefCell::new(Vec::new()));
        (
            Rc::new(Owner {
                text: "retained callback".into(),
                events: events.clone(),
            }),
            events,
        )
    }

    fn future(owner: Rc<Owner>, panic_poll: bool, panic_drop: bool) -> OwnerFuture<Owner, usize> {
        OwnerFuture::new(owner, |owner| {
            Box::pin(Borrowed {
                owner,
                address: Cell::new(std::ptr::null()),
                panic_poll,
                panic_drop,
                _pin: PhantomPinned,
            })
        })
    }

    #[test]
    fn moving_a_pending_call_preserves_its_future_and_capture_borrows() {
        let (owner, events) = owner();
        let weak = Rc::downgrade(&owner);
        let mut call = future(owner, false, false);
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(call.poll(&mut cx), Poll::Pending);
        let mut parked = vec![call];
        let mut call = parked.pop().unwrap();
        assert!(weak.upgrade().is_some());
        assert_eq!(call.poll(&mut cx), Poll::Ready("retained callback".len()));
        drop(call);
        assert!(weak.upgrade().is_none());
        assert_eq!(*events.borrow(), ["future", "owner"]);
    }

    #[test]
    fn separate_calls_retain_one_callback_until_the_last_cancellation() {
        let (owner, events) = owner();
        let mut first = future(owner.clone(), false, false);
        let mut second = future(owner, false, false);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(first.poll(&mut cx).is_pending());
        assert!(second.poll(&mut cx).is_pending());
        drop(first);
        assert_eq!(*events.borrow(), ["future"]);
        drop(second);
        assert_eq!(*events.borrow(), ["future", "future", "owner"]);
    }

    #[test]
    fn polling_and_destruction_panics_release_the_future_before_its_owner() {
        for (panic_poll, panic_drop) in [(true, false), (false, true)] {
            let (owner, events) = owner();
            assert!(catch_unwind(AssertUnwindSafe(|| {
                let mut call = future(owner, panic_poll, panic_drop);
                let _ = call.poll(&mut Context::from_waker(Waker::noop()));
            }))
            .is_err());
            assert_eq!(*events.borrow(), ["future", "owner"]);
        }
    }

    #[test]
    fn construction_panic_releases_the_owner() {
        let (owner, events) = owner();
        assert!(catch_unwind(AssertUnwindSafe(|| {
            let _: OwnerFuture<_, ()> = OwnerFuture::new(owner, |_| panic!("constructor panic"));
        }))
        .is_err());
        assert_eq!(*events.borrow(), ["owner"]);
    }
}
