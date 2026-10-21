#![cfg(target_arch = "wasm32")]

use relocade_wasm_runtime::Backend;
use relocade_wasm_runtime::WasmBackend;
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const EMPTY_MODULE: &[u8] = b"\0asm\x01\0\0\0";
const NAMED_MODULE: &[u8] = b"\0asm\x01\0\0\0\x00\x02\x01x";

async fn cached<T: 'static>(
    backend: &Backend<T>,
    bytes: &'static [u8],
) -> relocade_wasm_runtime::Module {
    backend
        .module_cache()
        .unwrap()
        .get_or_compile(bytes, |bytes| backend.compile(bytes))
        .await
        .unwrap()
}

#[wasm_bindgen_test]
async fn backends_share_cached_modules_but_keep_host_state() {
    let mut first = Backend::with_state(7).unwrap();
    let second = Backend::with_state("second").unwrap();
    let first_module = cached(&first, EMPTY_MODULE).await;
    let second_module = cached(&second, EMPTY_MODULE).await;
    assert_eq!(first_module, second_module);
    *first.state_mut() += 1;
    assert_eq!(*first.state(), 8);
    assert_eq!(*second.state(), "second");
}

#[wasm_bindgen_test]
async fn concurrent_requests_share_pending_compilation() {
    let backend = Backend::new().unwrap();
    let cache = relocade_wasm_runtime::ModuleCache::default();
    let mut first = pin!(cache.get_or_compile(EMPTY_MODULE, |bytes| backend.compile(bytes)));
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    let mut second = pin!(cache.get_or_compile(EMPTY_MODULE, |bytes| backend.compile(bytes)));
    assert!(second
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert_eq!(first.await.unwrap(), second.await.unwrap());
}

#[wasm_bindgen_test]
async fn different_assets_use_distinct_modules() {
    let backend = Backend::new().unwrap();
    let empty = cached(&backend, EMPTY_MODULE).await;
    let named = cached(&backend, NAMED_MODULE).await;
    assert_ne!(empty, named);
}

#[wasm_bindgen_test]
async fn ordinary_compilation_bypasses_the_shared_cache() {
    let backend = Backend::new().unwrap();
    let first = backend.compile(EMPTY_MODULE).await.unwrap();
    let retained = cached(&backend, EMPTY_MODULE).await;
    let second = backend.compile(EMPTY_MODULE).await.unwrap();
    assert_ne!(first, second);
    assert_ne!(first, retained);
    assert_ne!(second, retained);
}
