use relocade_workers::{Receiver, Thread, Transfer};
use std::{rc::Rc, sync::Arc};

struct Local(Rc<u32>);

impl Transfer for Local {
    type State = u32;
    type Payload = ();

    fn encode(self) -> (u32, ()) {
        (*self.0, ())
    }

    fn decode(value: u32, (): ()) -> Self {
        Self(Rc::new(value))
    }
}

#[derive(Transfer)]
struct Message<T> {
    value: T,
    metadata: Arc<Vec<u8>>,
    sequence: u32,
}

async fn rust_state() {
    // Vec is deliberately opaque here: sharing Arc's allocation does not encode
    // or reconstruct its referent.
    let data = Arc::new(vec![17, 29]);
    let expected = data.clone();
    let worker = Thread::new();
    let (mut input, task) = worker
        .listen(move |mut input: Receiver<Arc<Vec<u8>>>| async move {
            let data = input.recv().await.unwrap().unwrap();
            assert!(Arc::ptr_eq(&data, &expected));
            data[1]
        })
        .unwrap();
    input.send(data.clone()).unwrap();
    let value = task.await.unwrap();
    assert_eq!(value, 29);
    let expected = data.clone();
    let (mut input, task) = worker
        .listen(move |mut input: Receiver<Message<Local>>| async move {
            let message = input.recv().await.unwrap().unwrap();
            assert!(Arc::ptr_eq(&message.metadata, &expected));
            futures_util::future::ready(()).await;
            (*message.value.0, message.sequence)
        })
        .unwrap();
    input
        .send(Message {
            value: Local(Rc::new(42)),
            metadata: data,
            sequence: 7,
        })
        .unwrap();
    let value = task.await.unwrap();
    assert_eq!(value, (42, 7));
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn rust_state_keeps_identity_and_reconstructs_local_values() {
    pollster::block_on(rust_state());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn rust_state_keeps_identity_and_reconstructs_local_values() {
    rust_state().await;
}
