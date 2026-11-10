use super::*;
use relocade_workers::{AsJs, ChannelError, Post, Transfer, WorkerError};
use wasm_bindgen::JsValue;

struct Rejected;

impl AsJs for Rejected {
    fn to_js(&self) -> Result<JsValue, JsValue> {
        Err(JsValue::from_str("fixture rejects initial transfer"))
    }

    fn from_js(_: JsValue) -> Result<Self, JsValue> {
        unreachable!("the rejected payload cannot arrive")
    }
}

impl Post for Rejected {}

impl Transfer for Rejected {
    type State = ();
    type Payload = Self;

    fn encode(self) -> ((), Self) {
        ((), self)
    }

    fn decode((): (), payload: Self) -> Self {
        payload
    }
}

struct ReportDrop(Option<oneshot::Sender<ThreadId>>);

impl Drop for ReportDrop {
    fn drop(&mut self) {
        self.0.take().unwrap().send(thread::current().id()).unwrap();
    }
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn failed_initial_transfer_waits_for_worker_cleanup() {
    let (report, mut dropped) = oneshot::channel();
    let captured = ReportDrop(Some(report));
    let caller = thread::current().id();
    let worker = Worker::<()>::spawn_with(Rejected, move |_, _| async move {
        drop(captured);
        panic!("a failed transfer must not start its task");
    });
    let error = worker.join().await.unwrap_err();
    match error {
        WorkerError::Channel(ChannelError::Transport(message)) => {
            assert!(message.contains("fixture rejects initial transfer"));
        }
        other => panic!("expected the initial transfer error, got {other}"),
    }
    assert_ne!(dropped.try_recv().unwrap().unwrap(), caller);
}
