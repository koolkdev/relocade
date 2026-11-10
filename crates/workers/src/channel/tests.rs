use super::*;
use futures_channel::oneshot;
use futures_util::{
    future::{join, select, Either},
    FutureExt,
};
use std::{
    cell::RefCell,
    future::{pending, poll_fn},
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Poll,
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn receipt_policy() {
    let timers = Rc::new(RefCell::new(Vec::new()));
    let created = timers.clone();
    let signal = Signal(Arc::default());
    let (_host, endpoint) = crate::wake_connection(signal.clone());
    let executor = Executor::new(endpoint.bind(signal)).with_host_timer(
        Duration::from_millis(1),
        move |delay| {
            assert_eq!(delay, Duration::from_millis(1));
            let (complete, completed) = oneshot::channel();
            created.borrow_mut().push(complete);
            async move { completed.await.unwrap() }
        },
    );
    let (first_sender, first) = channel::<u32>().unwrap();
    let (second_sender, second) = channel::<u32>().unwrap();
    let mut first = first.with_executor(&executor);
    let mut second = second.with_executor(&executor);
    let (cancel_first, first_cancelled) = oneshot::channel();
    let (cancel_second, second_cancelled) = oneshot::channel();
    let (finish, finished) = oneshot::channel();
    let mut run = Box::pin(executor.run_until(pending::<()>(), async {
        join(
            async {
                assert!(matches!(
                    select(Box::pin(first.recv()), first_cancelled).await,
                    Either::Right(_)
                ));
            },
            async {
                assert!(matches!(
                    select(Box::pin(second.recv()), second_cancelled).await,
                    Either::Right(_)
                ));
            },
        )
        .await;
        finished.await.unwrap();
    }));
    assert!(run.as_mut().now_or_never().is_none());
    assert!(
        timers.borrow().is_empty(),
        "empty channels request no timer"
    );
    // Model two posted objects whose Rust state precedes the host callbacks.
    first_sender.states.unbounded_send(42).unwrap();
    second_sender.states.unbounded_send(73).unwrap();
    assert!(run.as_mut().now_or_never().is_none());
    assert_eq!(
        timers.borrow().len(),
        1,
        "receivers share one grace interval"
    );
    timers.borrow_mut().pop().unwrap().send(()).unwrap();
    // Delivery remains late after a real host turn; start another shared interval.
    poll_fn(|cx| {
        assert!(run.as_mut().poll(cx).is_pending());
        if timers.borrow().is_empty() {
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    })
    .await;
    assert_eq!(timers.borrow().len(), 1);

    cancel_first.send(()).unwrap();
    assert!(run.as_mut().now_or_never().is_none());
    assert_eq!(timers.borrow().len(), 1);
    assert!(
        !timers.borrow()[0].is_canceled(),
        "cancelling one receiver preserves the other's interval"
    );
    cancel_second.send(()).unwrap();
    assert!(run.as_mut().now_or_never().is_none());
    assert!(timers.borrow_mut().pop().unwrap().is_canceled());
    assert!(!executor.host.pending.has_pending());
    finish.send(()).unwrap();
    assert_eq!(run.await, Some(()));

    first.deliveries = None;
    second.deliveries = None;
    first_sender.payloads.send(()).unwrap();
    second_sender.payloads.send(()).unwrap();
    assert_eq!(first.recv().await.unwrap().unwrap(), 42);
    assert_eq!(second.recv().await.unwrap().unwrap(), 73);
}

#[derive(Clone)]
struct Signal(Arc<AtomicBool>);
impl crate::WakeTarget for Signal {
    fn wake(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn acknowledge(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

async fn interrupt_later_sibling() {
    let signal = Signal(Arc::default());
    let (_host, endpoint) = crate::wake_connection(signal.clone());
    let executor = Executor::new(endpoint.bind(signal.clone()));
    let (sender, receiver) = channel::<u32>().unwrap();
    let mut receiver = receiver.with_executor(&executor);
    sender.states.unbounded_send(73).unwrap();
    let mut posted = false;
    let guest = poll_fn(|cx| {
        if !std::mem::replace(&mut posted, true) {
            assert!(
                signal.0.load(Ordering::SeqCst),
                "later guest entry must see the host-turn interrupt"
            );
            sender.payloads.send(()).unwrap();
        }
        cx.waker().wake_by_ref();
        Poll::<()>::Pending
    });
    let value = executor
        .run_until(pending::<()>(), async {
            match select(pin!(receiver.recv()), pin!(guest)).await {
                Either::Left((received, _)) => received.unwrap().unwrap(),
                Either::Right(_) => unreachable!(),
            }
        })
        .await
        .unwrap();
    assert_eq!(value, 73);
}

async fn stop_during_host_turn() {
    let signal = Signal(Arc::default());
    let (_host, endpoint) = crate::wake_connection(signal.clone());
    let executor = Executor::new(endpoint.bind(signal));
    let (sender, receiver) = channel::<u32>().unwrap();
    let mut receiver = receiver.with_executor(&executor);
    sender.states.unbounded_send(42).unwrap();
    let (stop, stopped) = oneshot::channel();
    let mut run = Box::pin(executor.run_until(stopped, receiver.recv()));
    assert!(run.as_mut().now_or_never().is_none());
    stop.send(()).unwrap();
    assert!(
        run.await.is_none(),
        "stop must not wait for the missing payload"
    );
    assert!(!executor.host.pending.has_pending());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn stopping_during_a_host_turn_drops_the_receipt() {
    pollster::block_on(stop_during_host_turn());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn stopping_during_a_host_turn_drops_the_receipt() {
    stop_during_host_turn().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn pending_receivers_share_host_turn_timing_until_the_last_wait_ends() {
    pollster::block_on(receipt_policy());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn pending_receivers_share_host_turn_timing_until_the_last_wait_ends() {
    receipt_policy().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_host_turn_request_interrupts_a_later_synchronous_sibling() {
    pollster::block_on(interrupt_later_sibling());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_host_turn_request_interrupts_a_later_synchronous_sibling() {
    interrupt_later_sibling().await;
}
