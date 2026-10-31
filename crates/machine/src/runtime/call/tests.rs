use super::*;
use std::{
    cell::Cell,
    future::{pending, poll_fn, ready},
    task::Waker,
};

fn cx() -> Context<'static> {
    Context::from_waker(Waker::noop())
}
fn suspended(poll: CallPoll) -> Call {
    match poll {
        CallPoll::Pending(call) => call,
        CallPoll::Ready(_) => panic!("expected a pending service"),
    }
}
fn completed(poll: CallPoll) {
    assert!(matches!(poll, CallPoll::Ready(HookResult::Continue)));
}

#[test]
fn ready_awaits_finish_without_retaining_another_handler_owner() {
    let value = Rc::new(Cell::new(0));
    let observed = value.clone();
    let handler: Rc<dyn Handler> = Rc::new(async move || {
        value.set(ready(12).await);
        HookResult::Continue
    });
    completed(Call::start(&handler, &mut cx()));
    assert_eq!(observed.get(), 12);
    assert_eq!(Rc::strong_count(&handler), 1);
}

#[test]
fn nested_invocations_keep_locals_and_capture_borrows_until_selected() {
    let started = Rc::new(Cell::new(0));
    let count = started.clone();
    let result = Rc::new(Cell::new(0));
    let observed = result.clone();
    let text = String::from("retained capture");
    let handler: Rc<dyn Handler> = Rc::new(async move || {
        count.set(count.get() + 1);
        let local = [count.get()];
        let saved = &local[0];
        let capture = text.as_str();
        let mut first = true;
        poll_fn(|_| {
            if std::mem::replace(&mut first, false) {
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        })
        .await;
        assert_eq!(capture, "retained capture");
        result.set(*saved);
        HookResult::Continue
    });
    let outer = suspended(Call::start(&handler, &mut cx()));
    let inner = suspended(Call::start(&handler, &mut cx()));
    let weak = Rc::downgrade(&handler);
    drop(handler);
    let mut frames = vec![outer, inner];
    completed(frames.pop().unwrap().poll(&mut cx()));
    assert_eq!(observed.get(), 2);
    assert!(weak.upgrade().is_some());
    completed(frames.pop().unwrap().poll(&mut cx()));
    assert_eq!(observed.get(), 1);
    assert!(weak.upgrade().is_none());
}

struct BorrowedCleanup<'a> {
    capture: &'a str,
    drops: &'a Cell<u32>,
}
impl Drop for BorrowedCleanup<'_> {
    fn drop(&mut self) {
        assert_eq!(self.capture, "retained capture");
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn abandonment_drops_borrowed_locals_before_the_last_handler_owner() {
    let drops = Rc::new(Cell::new(0));
    let events = drops.clone();
    let text = String::from("retained capture");
    let handler: Rc<dyn Handler> = Rc::new(async move || {
        let _cleanup = BorrowedCleanup {
            capture: &text,
            drops: &events,
        };
        pending::<()>().await;
        panic!("an abandoned future must not resume");
    });
    let call = suspended(Call::start(&handler, &mut cx()));
    let weak = Rc::downgrade(&handler);
    drop(handler);
    drop(call);
    assert_eq!(drops.get(), 1);
    assert!(weak.upgrade().is_none());
}

#[test]
fn unwinding_first_and_later_polls_destroys_borrows_before_captures() {
    for pending_first in [false, true] {
        let drops = Rc::new(Cell::new(0));
        let events = drops.clone();
        let text = String::from("retained capture");
        let handler: Rc<dyn Handler> = Rc::new(async move || {
            let _cleanup = BorrowedCleanup {
                capture: &text,
                drops: &events,
            };
            let mut first = pending_first;
            poll_fn(|_| {
                if std::mem::replace(&mut first, false) {
                    Poll::Pending
                } else {
                    panic!("poll")
                }
            })
            .await
        });
        let weak = Rc::downgrade(&handler);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let poll = Call::start(&handler, &mut cx());
            drop(handler);
            if let CallPoll::Pending(call) = poll {
                call.poll(&mut cx());
            }
        }));
        assert!(result.is_err());
        assert_eq!(drops.get(), 1);
        assert!(weak.upgrade().is_none());
    }
}

struct PinnedCompletion {
    owner: std::rc::Weak<()>,
    drops: Rc<Cell<u32>>,
    address: Cell<usize>,
    first: Cell<bool>,
    panic_on_drop: bool,
    _pin: std::marker::PhantomPinned,
}
impl std::future::Future for PinnedCompletion {
    type Output = HookResult;
    fn poll(self: std::pin::Pin<&mut Self>, _: &mut Context<'_>) -> Poll<HookResult> {
        let address = &*self as *const Self as usize;
        let previous = self.address.replace(address);
        assert!(previous == 0 || previous == address);
        if self.first.replace(false) {
            Poll::Pending
        } else {
            Poll::Ready(HookResult::Continue)
        }
    }
}
impl Drop for PinnedCompletion {
    fn drop(&mut self) {
        assert!(self.owner.upgrade().is_some());
        assert_eq!(self.address.get(), self as *const Self as usize);
        self.drops.set(self.drops.get() + 1);
        if self.panic_on_drop {
            panic!("future destructor");
        }
    }
}

#[test]
fn custom_future_stays_pinned_and_drops_before_its_owner_even_when_drop_panics() {
    for pending_first in [false, true] {
        for panic_on_drop in [false, true] {
            let owner = Rc::new(());
            let released = Rc::downgrade(&owner);
            let drops = Rc::new(Cell::new(0));
            let observed = drops.clone();
            let handler: Rc<dyn Handler> = Rc::new(move || PinnedCompletion {
                owner: Rc::downgrade(&owner),
                drops: drops.clone(),
                address: Cell::new(0),
                first: Cell::new(pending_first),
                panic_on_drop,
                _pin: std::marker::PhantomPinned,
            });
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                let poll = Call::start(&handler, &mut cx());
                drop(handler);
                match poll {
                    CallPoll::Ready(result) => assert_eq!(result, HookResult::Continue),
                    CallPoll::Pending(call) => {
                        let mut moved = vec![call];
                        completed(moved.pop().unwrap().poll(&mut cx()));
                    }
                }
            }));
            assert_eq!(result.is_err(), panic_on_drop);
            assert_eq!(observed.get(), 1);
            assert!(released.upgrade().is_none());
        }
    }
}
