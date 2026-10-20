#![cfg(not(target_arch = "wasm32"))]

use relocade_runtime::wasm::WasmBackend;
use relocade_runtime_wasmtime::WasmtimeBackend;

const EMPTY_MODULE: &[u8] = b"\0asm\x01\0\0\0";
const NAMED_MODULE: &[u8] = b"\0asm\x01\0\0\0\x00\x02\x01x";

fn cached<T>(backend: &WasmtimeBackend<T>, bytes: &'static [u8]) -> wasmtime::Module {
    pollster::block_on(
        backend
            .module_cache()
            .unwrap()
            .get_or_compile(bytes, |bytes| backend.compile(bytes)),
    )
    .unwrap()
}

#[test]
fn concurrent_backends_share_cached_modules_but_keep_host_state() {
    let threads: Vec<_> = (0..4)
        .map(|state| {
            std::thread::spawn(move || {
                let mut backend = WasmtimeBackend::with_state(state).unwrap();
                let module = cached(&backend, EMPTY_MODULE);
                assert_eq!(*backend.state(), state);
                *backend.state_mut() += 10;
                assert_eq!(*backend.state(), state + 10);
                module
            })
        })
        .collect();
    let modules: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let backend = WasmtimeBackend::new().unwrap();
    let module = cached(&backend, EMPTY_MODULE);
    for other in modules {
        assert!(wasmtime::Module::same(&module, &other));
    }
}

#[test]
fn different_assets_use_distinct_modules() {
    let backend = WasmtimeBackend::new().unwrap();
    let empty = cached(&backend, EMPTY_MODULE);
    let named = cached(&backend, NAMED_MODULE);
    assert!(!wasmtime::Module::same(&empty, &named));
    assert!(wasmtime::Module::same(
        &empty,
        &cached(&backend, EMPTY_MODULE),
    ));
}

#[test]
fn ordinary_compilation_bypasses_the_shared_cache() {
    let backend = WasmtimeBackend::new().unwrap();
    let first = pollster::block_on(backend.compile(EMPTY_MODULE)).unwrap();
    let retained = cached(&backend, EMPTY_MODULE);
    let second = pollster::block_on(backend.compile(EMPTY_MODULE)).unwrap();
    assert!(!wasmtime::Module::same(&first, &second));
    assert!(!wasmtime::Module::same(&first, &retained));
    assert!(!wasmtime::Module::same(&second, &retained));
}

#[test]
fn failed_compilation_does_not_prevent_later_modules_from_loading() {
    let backend = WasmtimeBackend::new().unwrap();
    assert!(pollster::block_on(backend.compile(b"invalid Wasm")).is_err());
    assert!(pollster::block_on(backend.compile(EMPTY_MODULE)).is_ok());
}
