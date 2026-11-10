use relocade_workers::{Thread, ThreadExt, Transfer};
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

async fn rust_state() {
    // Vec is deliberately opaque here: sharing Arc's allocation does not encode
    // or reconstruct its referent.
    let data = Arc::new(vec![17, 29]);
    let expected = data.clone();
    let worker = Thread::new();
    let value = worker
        .run_resource(data, move |data| async move {
            assert!(Arc::ptr_eq(&data, &expected));
            data[1]
        })
        .await
        .unwrap();
    assert_eq!(value, 29);
    let value = worker
        .run_resource(Local(Rc::new(42)), |local| async move {
            futures_util::future::ready(()).await;
            *local.0
        })
        .await
        .unwrap();
    assert_eq!(value, 42);
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
