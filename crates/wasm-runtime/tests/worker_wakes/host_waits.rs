use super::{executor::CountWakes, *};
use futures_util::{stream::FuturesUnordered, StreamExt};
use relocade_wasm_runtime::worker::{wait_for_host, Executor, WakeHost};
use std::{
    cell::Cell,
    future::{pending, poll_fn, Future},
    pin::{pin, Pin},
    task::{Context, Poll},
};

fn executor() -> (WakeHost<Signal>, Executor<Signal>) {
    let (released, _release) = oneshot::channel();
    let signal = Signal::new(released);
    let (host, endpoint) = wake_connection(signal.clone());
    (host, Executor::new(endpoint.bind(signal)))
}

// Poll retained work under a chosen owner without awaiting its completion.
fn poll_on<F: Future>(
    executor: &Executor<Signal>,
    mut future: Pin<&mut F>,
    cx: &mut Context<'_>,
) -> Poll<F::Output> {
    let mut root = pin!(executor.run_until(
        pending::<()>(),
        poll_fn(|cx| Poll::Ready(future.as_mut().poll(cx))),
    ));
    match root.as_mut().poll(cx) {
        Poll::Ready(Some(result)) => result,
        _ => panic!("single poll must finish synchronously"),
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn ready_waits_and_repeated_pending_polls_do_not_hold_extra_registrations() {
    let (_host, executor) = executor();
    let mut entry = executor.execution_waiter();
    let mut cx = Context::from_waker(Waker::noop());
    let mut ready = pin!(wait_for_host(async { 42 }));
    assert_eq!(poll_on(&executor, ready.as_mut(), &mut cx), Poll::Ready(42));
    assert!(entry.poll_ready(&mut cx).is_ready());

    let (send, receive) = oneshot::channel::<()>();
    let mut wait = pin!(wait_for_host(receive));
    for _ in 0..3 {
        assert!(poll_on(&executor, wait.as_mut(), &mut cx).is_pending());
        assert!(entry.poll_ready(&mut cx).is_pending());
    }
    send.send(()).unwrap();
    assert_eq!(
        poll_on(&executor, wait.as_mut(), &mut cx),
        Poll::Ready(Ok(()))
    );
    assert!(entry.poll_ready(&mut cx).is_ready());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn last_completion_or_cancellation_wakes_the_cpu_child() {
    for cancel in [false, true] {
        let (_host, executor) = executor();
        let (first, one) = oneshot::channel::<()>();
        let (second, two) = oneshot::channel::<()>();
        let mut one = pin!(wait_for_host(one));
        let mut two = Box::pin(wait_for_host(two));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(poll_on(&executor, one.as_mut(), &mut cx).is_pending());
        assert!(poll_on(&executor, two.as_mut(), &mut cx).is_pending());
        let ran = Cell::new(false);
        let mut tasks = FuturesUnordered::new();
        tasks.push(async {
            let mut entry = executor.execution_waiter();
            poll_fn(|cx| entry.poll_ready(cx)).await;
            ran.set(true);
        });
        let mut run = pin!(tasks.next());
        assert!(run.as_mut().poll(&mut cx).is_pending());
        first.send(()).unwrap();
        assert!(poll_on(&executor, one.as_mut(), &mut cx).is_ready());
        assert!(run.as_mut().poll(&mut cx).is_pending());
        assert!(!ran.get(), "another independent host wait is still pending");
        if cancel {
            drop(two);
        } else {
            second.send(()).unwrap();
            assert!(poll_on(&executor, two.as_mut(), &mut cx).is_ready());
        }
        assert!(run.as_mut().poll(&mut cx).is_ready());
        assert!(
            ran.get(),
            "waking only the root would leave this child parked"
        );
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn replacing_the_cpu_waiter_releases_its_waker() {
    let (_host, executor) = executor();
    let mut wait = Box::pin(wait_for_host(pending::<()>()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(poll_on(&executor, wait.as_mut(), &mut cx).is_pending());
    let old = Arc::new(CountWakes::default());
    let weak = Arc::downgrade(&old);
    {
        let waker = Waker::from(old);
        let mut entry = executor.execution_waiter();
        assert!(entry
            .poll_ready(&mut Context::from_waker(&waker))
            .is_pending());
    }
    assert!(
        weak.upgrade().is_none(),
        "cancelled CPU waiter must be released"
    );
    let next = Arc::new(CountWakes::default());
    let waker = Waker::from(next.clone());
    let mut entry = executor.execution_waiter();
    assert!(entry
        .poll_ready(&mut Context::from_waker(&waker))
        .is_pending());
    drop(wait);
    assert_eq!(next.0.load(Ordering::SeqCst), 1);
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn retained_waits_follow_their_polling_owner() {
    let (_host_a, a) = executor();
    let (_host_b, b) = executor();
    let mut entry_a = a.execution_waiter();
    let mut entry_b = b.execution_waiter();
    let mut wait = pin!(wait_for_host(pending::<()>()));
    let mut cx = Context::from_waker(Waker::noop());
    assert!(poll_on(&a, wait.as_mut(), &mut cx).is_pending());
    assert!(entry_a.poll_ready(&mut cx).is_pending());
    assert!(entry_b.poll_ready(&mut cx).is_ready());
    assert!(poll_on(&b, wait.as_mut(), &mut cx).is_pending());
    assert!(entry_a.poll_ready(&mut cx).is_ready());
    assert!(entry_b.poll_ready(&mut cx).is_pending());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    assert!(
        entry_b.poll_ready(&mut cx).is_ready(),
        "outside an executor is an ordinary await"
    );
}
