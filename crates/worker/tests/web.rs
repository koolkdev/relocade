#![cfg(target_arch = "wasm32")]

use js_sys::{Array, Uint8Array, WebAssembly};
use relocade_worker::web::{Endpoint, Worker};
use relocade_worker::{Message, RequestEndpoint, RequestWorker};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};
use wasm_bindgen::prelude::*;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen(inline_js = "
export function test_url(path) { return new URL(path, location.href).href; }
export function shared_memory() { return new WebAssembly.Memory({initial: 1, maximum: 1, shared: true}); }
")]
extern "C" {
    fn test_url(path: &str) -> String;
    fn shared_memory() -> WebAssembly::Memory;
}

fn worker(entry: &str) -> (Worker, relocade_worker::web::Receiver) {
    Worker::spawn(
        &test_url("/wasm-bindgen-test"),
        &test_url("/wasm-bindgen-test_bg.wasm"),
        entry,
        &JsValue::NULL,
    )
    .unwrap()
}

#[wasm_bindgen]
pub async fn echo_worker(_: JsValue, mut endpoint: Endpoint, _: JsValue) -> Result<(), JsValue> {
    endpoint.ready();
    while let Some(message) = endpoint.recv().await.map_err(|error| error.to_string())? {
        endpoint
            .send(&message, &[])
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

#[wasm_bindgen]
pub async fn failed_worker(_: JsValue, endpoint: Endpoint, _: JsValue) -> Result<(), JsValue> {
    endpoint.ready();
    Err("entry failed".into())
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn accepted_messages_survive_early_shutdown_and_owner_drop() {
    let (mut worker, mut events) = worker("echo_worker");
    worker.send(&42.into(), &[]).unwrap();
    worker.send(&JsValue::NULL, &[]).unwrap();
    worker.shutdown();
    assert!(worker.send(&7.into(), &[]).is_err());
    drop(worker.ready());
    worker.ready().await.unwrap();
    let stopped = worker.stopped();
    drop(worker);
    assert_eq!(events.recv().await.unwrap().unwrap().as_f64(), Some(42.0));
    assert!(events.recv().await.unwrap().unwrap().is_null());
    assert!(events.recv().await.unwrap().is_none());
    stopped.await.unwrap();
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn cancelled_receive_preserves_the_message_and_transfer_is_explicit() {
    let (mut worker, mut events) = worker("echo_worker");
    worker.ready().await.unwrap();
    assert!(pin!(events.recv())
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    let bytes = Uint8Array::from(&[42u8][..]);
    worker.send(&bytes, &[]).unwrap();
    assert_eq!(bytes.length(), 1);
    assert_eq!(
        Uint8Array::new(&events.recv().await.unwrap().unwrap()).to_vec(),
        [42]
    );
    worker.send(&bytes, &[bytes.buffer().into()]).unwrap();
    assert_eq!(bytes.length(), 0);
    assert_eq!(
        Uint8Array::new(&events.recv().await.unwrap().unwrap()).to_vec(),
        [42]
    );
    worker.shutdown();
    worker.stopped().await.unwrap();
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn modules_and_shared_memory_cross_the_channel_without_serialization() {
    let (mut worker, mut events) = worker("echo_worker");
    let module = WebAssembly::Module::new(&Uint8Array::from(&b"\0asm\x01\0\0\0"[..])).unwrap();
    let memory = shared_memory();
    let payload = Array::of2(&module, &memory);
    worker.send(&payload, &[]).unwrap();
    let received = Array::from(&events.recv().await.unwrap().unwrap());
    assert!(received.get(0).is_instance_of::<WebAssembly::Module>());
    let other = received.get(1).unchecked_into::<WebAssembly::Memory>();
    Uint8Array::new(&other.buffer()).set_index(0, 42);
    assert_eq!(Uint8Array::new(&memory.buffer()).get_index(0), 42);
    worker.shutdown();
    worker.stopped().await.unwrap();
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn startup_and_entry_failure_wake_all_waiters() {
    let (missing, mut events) = Worker::spawn(
        &test_url("/missing-worker.js"),
        &test_url("/missing-worker.wasm"),
        "echo_worker",
        &JsValue::NULL,
    )
    .unwrap();
    let readiness = missing.ready();
    let stopped = missing.stopped();
    assert!(readiness.await.is_err());
    assert!(events.recv().await.is_err());
    assert!(stopped.await.is_err());
    assert!(missing.send(&JsValue::NULL, &[]).is_err());

    let (failed, mut events) = worker("failed_worker");
    failed.ready().await.unwrap();
    assert!(events
        .recv()
        .await
        .unwrap_err()
        .to_string()
        .contains("entry failed"));
    assert!(failed
        .stopped()
        .await
        .unwrap_err()
        .to_string()
        .contains("entry failed"));
}

#[wasm_bindgen]
pub async fn request_worker(_: JsValue, endpoint: Endpoint, _: JsValue) -> Result<(), JsValue> {
    let endpoint: RequestEndpoint<JsValue, JsValue> = endpoint.into();
    endpoint
        .serve(async |value: JsValue| {
            if value.as_f64() == Some(-1.0) {
                Err(relocade_worker::Error::Failed("request rejected".into()))
            } else {
                Ok(value)
            }
        })
        .await
        .map_err(|error| error.to_string().into())
}

fn request_client(
    worker: (Worker, relocade_worker::web::Receiver),
    limit: usize,
) -> RequestWorker<JsValue, JsValue> {
    RequestWorker::from_worker(worker, limit)
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn request_limits_and_dropped_waiters_preserve_reply_order() {
    let mut requests = request_client(worker("request_worker"), 2);
    let discarded = requests.submit(1.into()).unwrap().unwrap();
    let accepted = requests.submit(42.into()).unwrap().unwrap();
    drop(discarded);
    assert!(requests.submit(3.into()).unwrap().is_none());
    requests.shutdown();
    assert!(requests.submit(4.into()).is_err());
    let stopped = requests.stopped();
    drop(requests);
    assert_eq!(accepted.await.unwrap().as_f64(), Some(42.0));
    stopped.await.unwrap();
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn failed_delivery_releases_capacity_and_terminal_failure_completes_all_requests() {
    let mut requests = request_client(worker("request_worker"), 1);
    // Functions cannot be structured-cloned. A rejected send must not occupy
    // the only slot or leave a reply that consumes the next request's result.
    let function = js_sys::Reflect::get(&js_sys::global(), &"setTimeout".into()).unwrap();
    assert!(requests.submit(function).is_err());
    let rejected = requests.submit((-1).into()).unwrap().unwrap();
    assert!(rejected
        .await
        .unwrap_err()
        .to_string()
        .contains("request rejected"));
    let first = requests.submit(7.into()).unwrap().unwrap();
    assert_eq!(first.await.unwrap().as_f64(), Some(7.0));
    assert_eq!(
        requests
            .submit(8.into())
            .unwrap()
            .unwrap()
            .await
            .unwrap()
            .as_f64(),
        Some(8.0)
    );
    requests.shutdown();
    requests.stopped().await.unwrap();

    let mut requests = request_client(worker("failed_worker"), 2);
    let first = requests.submit(1.into()).unwrap().unwrap();
    let second = requests.submit(2.into()).unwrap().unwrap();
    for result in [first.await, second.await] {
        assert!(result.unwrap_err().to_string().contains("entry failed"));
    }
}

#[derive(Message)]
struct Payload {
    optional: Option<Option<JsValue>>,
    bytes: Vec<u8>,
    module: WebAssembly::Module,
}

#[wasm_bindgen]
pub async fn payload_worker(_: JsValue, endpoint: Endpoint, _: JsValue) -> Result<(), JsValue> {
    let endpoint: RequestEndpoint<Payload, Payload> = endpoint.into();
    endpoint
        .serve(async |value| Ok(value))
        .await
        .map_err(|error| error.to_string().into())
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn typed_messages_preserve_nested_options_bytes_and_module_handles() {
    let module = WebAssembly::Module::new(&Uint8Array::from(&b"\0asm\x01\0\0\0"[..])).unwrap();
    let mut client = RequestWorker::<Payload, Payload>::from_worker(worker("payload_worker"), 1);
    for optional in [None, Some(None), Some(Some(JsValue::NULL))] {
        let expected = optional.clone();
        let returned = client
            .submit(Payload {
                optional,
                bytes: vec![7, 42],
                module: module.clone(),
            })
            .unwrap()
            .unwrap()
            .await
            .unwrap();
        match (expected, returned.optional) {
            (None, None) | (Some(None), Some(None)) => {}
            (Some(Some(expected)), Some(Some(actual))) => assert_eq!(actual, expected),
            _ => panic!("nested option changed in transit"),
        }
        assert_eq!(returned.bytes, [7, 42]);
        assert!(JsValue::from(returned.module).is_instance_of::<WebAssembly::Module>());
    }
    client.shutdown();
    client.stopped().await.unwrap();
}

#[wasm_bindgen_test::wasm_bindgen_test]
async fn composed_messages_transfer_each_owned_buffer() {
    let (mut worker, mut events) = worker("echo_worker");
    let packet = (vec![1u8, 2], vec![42u8]).encode();
    assert_eq!(packet.transfer.len(), 2);
    worker.send(&packet.value, &packet.transfer).unwrap();
    for buffer in packet.transfer {
        assert_eq!(
            buffer.unchecked_into::<js_sys::ArrayBuffer>().byte_length(),
            0
        );
    }
    let decoded = <(Vec<u8>, Vec<u8>)>::decode(events.recv().await.unwrap().unwrap()).unwrap();
    assert_eq!(decoded, (vec![1, 2], vec![42]));
    worker.shutdown();
    worker.stopped().await.unwrap();
}
