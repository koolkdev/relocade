use super::*;
use futures_channel::oneshot;
use futures_util::FutureExt;
use std::{
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[derive(Clone, Default)]
struct Signal(Arc<AtomicBool>);
impl crate::WakeTarget for Signal {
    fn wake(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn acknowledge(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

async fn pending_receipts() {
    let signal = Signal::default();
    let (_host, endpoint) = crate::wake_connection(signal.clone());
    let worker = WorkerWake::new(endpoint.bind(signal.clone()));
    let host = worker.host_turns();
    let (first_sender, first) = channel::<u32>().unwrap();
    let (second_sender, second) = channel::<u32>().unwrap();
    let mut first = first.with_worker(&worker);
    let mut second = second.with_worker(&worker);
    {
        let mut a = Box::pin(first.recv());
        let mut b = Box::pin(second.recv());
        assert!(a.as_mut().now_or_never().is_none());
        assert!(!host.has_pending(), "an empty channel needs no host turn");

        // Rust state can arrive before its posted browser payload.
        first_sender.states.unbounded_send(42).unwrap();
        second_sender.states.unbounded_send(73).unwrap();
        worker.poll(|cx| {
            assert!(a.as_mut().poll(cx).is_pending());
            assert!(signal.0.load(Ordering::SeqCst), "interrupt busy work");
            assert!(b.as_mut().poll(cx).is_pending());
        });
        assert!(host.has_pending());
        drop(a);
        assert!(host.has_pending(), "the second receipt is still waiting");
    }
    assert!(!host.has_pending());
    first_sender.payloads.send(()).unwrap();
    second_sender.payloads.send(()).unwrap();
    assert_eq!(first.recv().await.unwrap().unwrap(), 42);
    assert_eq!(second.recv().await.unwrap().unwrap(), 73);
    assert!(!host.has_pending());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelling_receipts_preserves_their_messages_and_other_host_waits() {
    pollster::block_on(pending_receipts());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelling_receipts_preserves_their_messages_and_other_host_waits() {
    pending_receipts().await;
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn only_delivery_requests_a_new_poll_for_an_idle_host_dependency() {
    let signal = Signal::default();
    let (_host, endpoint) = crate::wake_connection(signal.clone());
    let worker = WorkerWake::new(endpoint.bind(signal));
    let host = worker.host_turns();
    let (complete, operation) = oneshot::channel::<()>();
    let mut wait = pin!(host.wait(operation));
    assert!(worker.poll(|cx| wait.as_mut().poll(cx)).is_pending());
    assert!(host.has_pending());
    assert!(!worker.has_pending_poll(), "the worker can park");
    complete.send(()).unwrap();
    assert!(worker.wait().now_or_never().is_some());
    assert_eq!(
        worker.poll(|cx| wait.as_mut().poll(cx)),
        std::task::Poll::Ready(Ok(()))
    );
    assert!(!host.has_pending());
}
