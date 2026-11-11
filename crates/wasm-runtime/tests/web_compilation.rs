#![cfg(target_arch = "wasm32")]

use relocade_wasm_runtime::{WasmBackend, WasmCompiler, WasmRuntime};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Waker},
};
use wasm_bindgen_test::wasm_bindgen_test;

wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const EMPTY_MODULE: &[u8] = b"\0asm\x01\0\0\0";

async fn cached(runtime: &WasmRuntime, bytes: &'static [u8]) -> relocade_wasm_runtime::Module {
    let compiler = runtime.compiler();
    runtime
        .module_cache()
        .unwrap()
        .get_or_compile(bytes, |bytes| compiler.compile(bytes))
        .await
        .unwrap()
}

#[wasm_bindgen_test]
async fn runtimes_share_cached_modules() {
    let first = WasmRuntime::new().unwrap();
    let second = WasmRuntime::new().unwrap();
    let first_module = cached(&first, EMPTY_MODULE).await;
    let second_module = cached(&second, EMPTY_MODULE).await;
    assert_eq!(first_module, second_module);
}

#[wasm_bindgen_test]
async fn concurrent_requests_share_pending_compilation() {
    let runtime = WasmRuntime::new().unwrap();
    let compiler = runtime.compiler();
    let cache = relocade_wasm_runtime::ModuleCache::default();
    let mut first = pin!(cache.get_or_compile(EMPTY_MODULE, |bytes| compiler.compile(bytes)));
    assert!(first
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    let mut second = pin!(cache.get_or_compile(EMPTY_MODULE, |bytes| compiler.compile(bytes)));
    assert!(second
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert_eq!(first.await.unwrap(), second.await.unwrap());
}

#[wasm_bindgen_test]
async fn ordinary_compilation_bypasses_the_shared_cache() {
    let runtime = WasmRuntime::new().unwrap();
    let first = runtime.compiler().compile(EMPTY_MODULE).await.unwrap();
    let retained = cached(&runtime, EMPTY_MODULE).await;
    let second = runtime.compiler().compile(EMPTY_MODULE).await.unwrap();
    assert_ne!(first, second);
    assert_ne!(first, retained);
    assert_ne!(second, retained);
}
