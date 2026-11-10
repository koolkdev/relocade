use super::*;
use futures_util::FutureExt;

struct Object {
    value: u32,
    label: String,
}

impl WorkerResource for Object {
    type Posted = u32;

    fn prepare(self) -> ResourceTransfer<u32, impl FnOnce(u32) -> Self + Send> {
        ResourceTransfer::new(self.value, move |value| Self {
            value,
            label: self.label,
        })
    }
}

async fn cancelled_receipt() {
    let (mut sender, mut receiver) = channel::<Object>().unwrap();
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
fn receiving_on_another_owner_does_not_consume_the_object() {
    pollster::block_on(async {
        let worker = Worker::new();
        let (mut sender, mut receiver) = worker.channel::<Object>().await.unwrap();
        sender
            .send(Object {
                value: 42,
                label: "retained".into(),
            })
            .unwrap();
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            receiver.recv().now_or_never()
        }))
        .is_err());
        let received = worker
            .run(move || async move {
                let object = receiver.recv().await.unwrap().unwrap();
                (object.value, object.label)
            })
            .await
            .unwrap();
        assert_eq!(received, (42, "retained".into()));
    });
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
            let value = serde_wasm_bindgen::from_value(value)?;
            if value == 1 {
                Err(JsValue::from_str("rejected received payload"))
            } else {
                Ok(Self(value))
            }
        }
    }

    impl Post for Payload {}

    impl WorkerResource for Payload {
        type Posted = Self;

        fn prepare(self) -> ResourceTransfer<Self, impl FnOnce(Self) -> Self + Send> {
            ResourceTransfer::new(self, std::convert::identity)
        }
    }

    #[derive(crate::WorkerResource)]
    #[worker(crate = crate)]
    struct Message {
        #[worker(resource)]
        value: Payload,
        label: String,
    }

    #[wasm_bindgen_test::wasm_bindgen_test]
    async fn failed_posts_and_decoding_preserve_the_next_objects_metadata() {
        let (mut sender, mut receiver) = channel::<Message>().unwrap();
        assert!(sender
            .send(Message {
                value: Payload(0),
                label: "rejected".into()
            })
            .is_err());
        sender
            .send(Message {
                value: Payload(1),
                label: "invalid on receipt".into(),
            })
            .unwrap();
        sender
            .send(Message {
                value: Payload(73),
                label: "accepted".into(),
            })
            .unwrap();
        drop(sender);
        assert!(receiver.recv().await.unwrap().is_err());
        let message = receiver.recv().await.unwrap().unwrap();
        assert_eq!((message.value.0, message.label.as_str()), (73, "accepted"));
        assert!(receiver.recv().await.is_none());
    }
}
