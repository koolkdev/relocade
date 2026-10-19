#![cfg(not(target_arch = "wasm32"))]

use relocade_runtime::wasm::WasmBackend;
use relocade_runtime_wasmtime::WasmtimeBackend;

const EMPTY_MODULE: &[u8] = b"\0asm\x01\0\0\0";

#[test]
fn concurrent_backends_share_code_but_keep_host_state() {
    let threads: Vec<_> = (0..4)
        .map(|state| {
            std::thread::spawn(move || {
                let mut backend = WasmtimeBackend::with_state(state).unwrap();
                let module = pollster::block_on(backend.compile_shared(EMPTY_MODULE)).unwrap();
                assert_eq!(*backend.state(), state);
                *backend.state_mut() += 10;
                assert_eq!(*backend.state(), state + 10);
                module
            })
        })
        .collect();
    let modules: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let backend = WasmtimeBackend::new().unwrap();
    let module = pollster::block_on(backend.compile_shared(EMPTY_MODULE)).unwrap();
    for other in modules {
        assert!(wasmtime::Module::same(&module, &other));
    }
}

#[test]
fn failed_compilation_does_not_prevent_later_assets_from_loading() {
    let backend = WasmtimeBackend::new().unwrap();
    assert!(pollster::block_on(backend.compile_shared(b"invalid Wasm")).is_err());
    assert!(pollster::block_on(backend.compile_shared(EMPTY_MODULE)).is_ok());
}
