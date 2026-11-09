use super::*;
use futures_util::FutureExt;

#[derive(crate::WorkerResource)]
#[worker(crate = crate)]
struct Object {
    #[worker(post)]
    value: u32,
    label: String,
}

async fn cancelled_receipt() {
    let (mut sender, endpoint) = channel::<Object>().unwrap();
    let mut receiver = endpoint.bind();
    let ResourceTransfer { posted, rebuild } = Object {
        value: 42,
        label: "first".into(),
    }
    .prepare();
    // Delay the engine payload to exercise the browser's possible arrival order
    // on either platform: Rust state is ready while posted delivery is pending.
    sender
        .rebuild
        .unbounded_send(Box::new(rebuild))
        .ok()
        .unwrap();
    assert!(receiver.recv().now_or_never().is_none());
    sender.posted.send(posted).unwrap();
    sender
        .send(Object {
            value: 73,
            label: "second".into(),
        })
        .unwrap();
    drop(sender);
    let first = receiver.recv().await.unwrap().unwrap();
    let second = receiver.recv().await.unwrap().unwrap();
    assert_eq!((first.value, first.label.as_str()), (42, "first"));
    assert_eq!((second.value, second.label.as_str()), (73, "second"));
    assert!(receiver.recv().await.is_none());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_receipt_keeps_pending_rust_state_with_its_payload() {
    pollster::block_on(cancelled_receipt());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelled_receipt_keeps_pending_rust_state_with_its_payload() {
    cancelled_receipt().await;
}

#[cfg(target_arch = "wasm32")]
mod posting_errors {
    use super::*;
    use crate::{AsJs, Post};
    use wasm_bindgen::JsValue;

    struct Payload(u32);

    impl AsJs for Payload {
        fn to_js(&self) -> Result<JsValue, JsValue> {
            if self.0 == 0 {
                Err(JsValue::from_str("rejected payload"))
            } else {
                Ok(self.0.into())
            }
        }

        fn from_js(value: JsValue) -> Result<Self, JsValue> {
            serde_wasm_bindgen::from_value(value)
                .map(Self)
                .map_err(Into::into)
        }
    }

    impl Post for Payload {}

    #[derive(crate::WorkerResource)]
    #[worker(crate = crate)]
    struct Message {
        #[worker(post)]
        value: Payload,
        label: String,
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn a_failed_post_does_not_displace_the_next_objects_metadata() {
        let (mut sender, endpoint) = channel::<Message>().unwrap();
        let mut receiver = endpoint.bind();
        assert!(sender
            .send(Message {
                value: Payload(0),
                label: "rejected".into()
            })
            .is_err());
        sender
            .send(Message {
                value: Payload(73),
                label: "accepted".into(),
            })
            .unwrap();
        drop(sender);
        let message = receiver.recv().await.unwrap().unwrap();
        assert_eq!((message.value.0, message.label.as_str()), (73, "accepted"));
        assert!(receiver.recv().await.is_none());
    }
}

async fn receipt_policy() {
    use futures_channel::oneshot;
    use std::{cell::RefCell, time::Duration};

    let timers = Rc::new(RefCell::new(Vec::new()));
    let created = timers.clone();
    let host = Rc::new(
        HostTurns::new(std::task::Waker::noop().clone()).with_timer(move |delay| {
            assert_eq!(delay, Duration::from_millis(1));
            let (complete, completed) = oneshot::channel();
            created.borrow_mut().push(complete);
            async move { completed.await.unwrap() }
        }),
    );
    let (sender, endpoint) = channel::<Object>().unwrap();
    let mut receiver = endpoint.bind();
    receiver.host = Some(host.clone());
    assert!(receiver.recv().now_or_never().is_none());
    assert!(
        timers.borrow().is_empty(),
        "an empty channel schedules no timer"
    );
    assert_eq!(host.receive(async { 7 }).await, 7);
    assert!(
        timers.borrow().is_empty(),
        "ready receipt schedules no timer"
    );

    let ResourceTransfer { posted, rebuild } = Object {
        value: 42,
        label: "delayed".into(),
    }
    .prepare();
    sender
        .rebuild
        .unbounded_send(Box::new(rebuild))
        .ok()
        .unwrap();
    let mut receive = Box::pin(receiver.recv());
    assert!(receive.as_mut().now_or_never().is_none());
    assert_eq!(timers.borrow().len(), 1);
    assert!(
        !host.has_waiters(),
        "the grace interval precedes the host turn"
    );
    timers.borrow_mut().pop().unwrap().send(()).unwrap();
    assert!(receive.as_mut().now_or_never().is_none());
    assert!(host.has_waiters());
    host.resume();
    assert!(receive.as_mut().now_or_never().is_none());
    assert_eq!(
        timers.borrow().len(),
        1,
        "each retry has another grace interval"
    );
    drop(receive);
    assert!(timers.borrow_mut().pop().unwrap().is_canceled());
    assert!(!host.has_waiters());

    // Cancellation after the interval also releases an unconsumed host turn.
    let mut receive = Box::pin(receiver.recv());
    assert!(receive.as_mut().now_or_never().is_none());
    timers.borrow_mut().pop().unwrap().send(()).unwrap();
    assert!(receive.as_mut().now_or_never().is_none());
    assert!(host.has_waiters());
    drop(receive);
    assert!(!host.has_waiters());

    // The pending object survives both cancellations; finish on an ordinary host.
    receiver.host = None;
    sender.posted.send(posted).unwrap();
    let object = receiver.recv().await.unwrap().unwrap();
    assert_eq!((object.value, object.label.as_str()), (42, "delayed"));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn posted_receipt_owns_the_grace_interval_and_cancels_its_host_waits() {
    pollster::block_on(receipt_policy());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn posted_receipt_owns_the_grace_interval_and_cancels_its_host_waits() {
    receipt_policy().await;
}

async fn receipt_interrupts_later_sibling() {
    use futures_util::future::{select, Either};
    use std::{
        future::pending,
        pin::pin,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        task::Poll,
    };

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

    let signal = Signal(Arc::default());
    let (_host, endpoint) = crate::wake_connection(signal.clone());
    let executor = crate::Executor::new(endpoint.bind(signal.clone()));
    let (sender, endpoint) = channel::<Object>().unwrap();
    let mut receiver = endpoint.bind_on(&executor);
    let ResourceTransfer { posted, rebuild } = Object {
        value: 73,
        label: "ready metadata".into(),
    }
    .prepare();
    sender
        .rebuild
        .unbounded_send(Box::new(rebuild))
        .ok()
        .unwrap();
    let mut posted = Some(posted);
    let guest = poll_fn(|cx| {
        if let Some(posted) = posted.take() {
            // This sibling runs after receipt asks for a turn, but before the
            // executor can inspect its waiters. A synchronous guest must see
            // the interrupt here or it could prevent that host turn forever.
            assert!(signal.0.load(Ordering::SeqCst));
            sender.posted.send(posted).unwrap();
        }
        cx.waker().wake_by_ref();
        Poll::<()>::Pending
    });
    let object = executor
        .run_until(pending::<()>(), async {
            match select(pin!(receiver.recv()), pin!(guest)).await {
                Either::Left((received, _)) => received.unwrap().unwrap(),
                Either::Right(_) => unreachable!(),
            }
        })
        .await
        .unwrap();
    assert_eq!(object.value, 73);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_receipt_turn_interrupts_a_later_synchronous_sibling() {
    pollster::block_on(receipt_interrupts_later_sibling());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn a_receipt_turn_interrupts_a_later_synchronous_sibling() {
    receipt_interrupts_later_sibling().await;
}
