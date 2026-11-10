use super::*;
use futures_channel::oneshot;
use futures_util::{
    future::{select, Either},
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
    task::{Poll, Waker},
    time::Duration,
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn receipt_policy() {
    let timers = Rc::new(RefCell::new(Vec::new()));
    let created = timers.clone();
    let host = Rc::new(HostTurns::new(Waker::noop().clone()).with_timer(
        Duration::from_millis(1),
        move |delay| {
            assert_eq!(delay, Duration::from_millis(1));
            let (complete, completed) = oneshot::channel();
            created.borrow_mut().push(complete);
            async move { completed.await.unwrap() }
        },
    ));
    let (sender, mut receiver) = channel::<u32>().unwrap();
    receiver.host = Some(host.clone());
    assert!(receiver.recv().now_or_never().is_none());
    assert!(
        timers.borrow().is_empty(),
        "empty channels request no timer"
    );
    assert_eq!(host.receive(async { 7 }).await, 7);
    assert!(timers.borrow().is_empty(), "ready objects request no timer");

    // Model a posted object whose Rust state arrives before the host callback.
    sender.states.unbounded_send(42).unwrap();
    let mut receive = Box::pin(receiver.recv());
    assert!(receive.as_mut().now_or_never().is_none());
    assert_eq!(timers.borrow().len(), 1);
    assert!(
        !host.has_waiters(),
        "guest work continues during the grace interval"
    );
    timers.borrow_mut().pop().unwrap().send(()).unwrap();
    assert!(receive.as_mut().now_or_never().is_none());
    assert!(host.has_waiters());
    host.resume();
    assert!(receive.as_mut().now_or_never().is_none());
    assert_eq!(
        timers.borrow().len(),
        1,
        "late delivery gets another work interval"
    );
    drop(receive);
    assert!(timers.borrow_mut().pop().unwrap().is_canceled());
    assert!(!host.has_waiters());

    let mut receive = Box::pin(receiver.recv());
    assert!(receive.as_mut().now_or_never().is_none());
    timers.borrow_mut().pop().unwrap().send(()).unwrap();
    assert!(receive.as_mut().now_or_never().is_none());
    assert!(host.has_waiters());
    drop(receive);
    assert!(
        !host.has_waiters(),
        "cancelled receipts do not demand a host turn"
    );

    receiver.host = None;
    sender.payloads.send(()).unwrap();
    assert_eq!(receiver.recv().await.unwrap().unwrap(), 42);
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
    assert!(!executor.host_turns.has_waiters());
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
fn posted_receipt_waits_between_host_turns_and_cancels_its_timers() {
    pollster::block_on(receipt_policy());
}
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn posted_receipt_waits_between_host_turns_and_cancels_its_timers() {
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
