use super::*;
use relocade_workers::{AsJs, Post};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::wasm_bindgen_test;

#[wasm_bindgen_test]
async fn cancelled_receive_keeps_state_while_the_payload_is_in_flight() {
    let metadata = Arc::new(vec![2]);
    let (mut sender, mut receiver) = channel::<Message>().unwrap();
    sender.send(message(5, 17, metadata.clone())).unwrap();
    // Rust state is already available, but this thread has not given the browser
    // an event-loop turn to deliver its corresponding payload.
    assert!(receiver.recv().now_or_never().is_none());
    sender.send(message(9, 23, metadata.clone())).unwrap();
    drop(sender);
    for expected in [(5, 17), (9, 23)] {
        let received = receiver.recv().await.unwrap().unwrap();
        assert_eq!((received.sequence, *received.local.0), expected);
        assert!(Arc::ptr_eq(&received.metadata, &metadata));
    }
    assert!(receiver.recv().await.is_none());
}

struct Payload(u32);

impl AsJs for Payload {
    fn to_js(&self) -> Result<JsValue, JsValue> {
        if self.0 == 0 {
            return Err(JsValue::from_str("fixture rejects sending zero"));
        }
        self.0.to_js()
    }

    fn from_js(value: JsValue) -> Result<Self, JsValue> {
        let value = u32::from_js(value)?;
        if value == 1 {
            return Err(JsValue::from_str("fixture rejects receiving one"));
        }
        Ok(Self(value))
    }
}

impl Post for Payload {}

struct Checked {
    sequence: u32,
    payload: u32,
}

impl Transfer for Checked {
    type State = u32;
    type Payload = Payload;

    fn encode(self) -> (u32, Payload) {
        (self.sequence, Payload(self.payload))
    }

    fn decode(sequence: u32, payload: Payload) -> Self {
        Self {
            sequence,
            payload: payload.0,
        }
    }
}

#[wasm_bindgen_test]
async fn transport_errors_preserve_the_following_messages_pairing() {
    let (mut sender, mut receiver) = channel::<Checked>().unwrap();
    assert!(matches!(
        sender.send(Checked {
            sequence: 3,
            payload: 0
        }),
        Err(ChannelError::Transport(_)),
    ));
    sender
        .send(Checked {
            sequence: 7,
            payload: 1,
        })
        .unwrap();
    sender
        .send(Checked {
            sequence: 11,
            payload: 19,
        })
        .unwrap();
    drop(sender);
    assert!(matches!(
        receiver.recv().await,
        Some(Err(ChannelError::Transport(_)))
    ));
    let received = receiver.recv().await.unwrap().unwrap();
    assert_eq!((received.sequence, received.payload), (11, 19));
    assert!(receiver.recv().await.is_none());
}
