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
