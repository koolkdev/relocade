use futures_util::FutureExt;
use relocade_workers::{channel, ChannelError, Sender, Thread, ThreadExt, Transfer};
use std::{rc::Rc, sync::Arc};

#[cfg(target_arch = "wasm32")]
#[path = "channel/web.rs"]
mod web;

struct Local(Rc<u32>);

impl Transfer for Local {
    type State = ();
    type Payload = u32;

    fn encode(self) -> ((), u32) {
        ((), *self.0)
    }

    fn decode((): (), payload: u32) -> Self {
        Self(Rc::new(payload))
    }
}

#[derive(Transfer)]
struct Message {
    sequence: u32,
    local: Local,
    metadata: Arc<Vec<u8>>,
}

fn message(sequence: u32, value: u32, metadata: Arc<Vec<u8>>) -> Message {
    Message {
        sequence,
        local: Local(Rc::new(value)),
        metadata,
    }
}

async fn ordered_delivery() {
    let metadata = Arc::new(vec![17, 29]);
    let expected = metadata.clone();
    let (sender, mut receiver) = channel::<Message>().unwrap();
    assert!(receiver.recv().now_or_never().is_none());
    let worker = Thread::new();
    let task = worker.run_resource(sender, move |mut sender| async move {
        sender.send(message(3, 41, metadata.clone())).unwrap();
        sender.send(message(7, 83, metadata)).unwrap();
        // Queued browser messages must survive the sending endpoint's closure.
        drop(sender);
    });
    for (sequence, value) in [(3, 41), (7, 83)] {
        let received = receiver.recv().await.unwrap().unwrap();
        assert_eq!((received.sequence, *received.local.0), (sequence, value));
        assert!(Arc::ptr_eq(&received.metadata, &expected));
    }
    assert!(receiver.recv().await.is_none());
    task.await.unwrap();
}

async fn reverse_channel() {
    let (ready, mut incoming) = channel::<Sender<Message>>().unwrap();
    let metadata = Arc::new(vec![5, 9]);
    let expected = metadata.clone();
    let worker = Thread::new();
    let task = worker.run_resource(ready, move |mut ready| async move {
        // The receiving worker creates its local inbox, then sends the producer
        // endpoint back. No task-result transport or receiver registration is needed.
        let (sender, mut receiver) = channel::<Message>().unwrap();
        ready.send(sender).unwrap();
        drop(ready);
        let received = receiver.recv().await.unwrap().unwrap();
        assert!(Arc::ptr_eq(&received.metadata, &expected));
        assert!(receiver.recv().await.is_none());
        (received.sequence, *received.local.0)
    });
    let mut sender = incoming.recv().await.unwrap().unwrap();
    assert!(incoming.recv().await.is_none());
    sender.send(message(11, 97, metadata)).unwrap();
    drop(sender);
    assert_eq!(task.await.unwrap(), (11, 97));
}

fn closed_receiver() {
    let metadata = Arc::new(vec![1]);
    let (mut sender, receiver) = channel::<Message>().unwrap();
    sender.send(message(1, 2, metadata.clone())).unwrap();
    drop(receiver);
    assert_eq!(Arc::strong_count(&metadata), 1);
    assert!(matches!(
        sender.send(message(3, 4, metadata.clone())),
        Err(ChannelError::Closed),
    ));
    assert_eq!(Arc::strong_count(&metadata), 1);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn messages_keep_order_and_identity_through_sender_shutdown() {
    pollster::block_on(ordered_delivery());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn transferred_senders_establish_the_reverse_direction() {
    pollster::block_on(reverse_channel());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn dropping_a_receiver_releases_state_and_closes_sends() {
    closed_receiver();
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn messages_keep_order_and_identity_through_sender_shutdown() {
    ordered_delivery().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn transferred_senders_establish_the_reverse_direction() {
    reverse_channel().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
fn dropping_a_receiver_releases_state_and_closes_sends() {
    closed_receiver();
}
