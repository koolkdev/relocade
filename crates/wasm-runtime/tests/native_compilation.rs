#![cfg(not(target_arch = "wasm32"))]

use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};

const EMPTY_MODULE: &[u8] = b"\0asm\x01\0\0\0";

fn cached<T>(backend: &Backend<T>, bytes: &'static [u8]) -> wasmtime::Module {
    let compiler = backend.compiler();
    pollster::block_on(
        backend
            .module_cache()
            .unwrap()
            .get_or_compile(bytes, |bytes| compiler.compile(bytes)),
    )
    .unwrap()
}

#[test]
fn concurrent_backends_share_cached_modules_but_keep_host_state() {
    let threads: Vec<_> = (0..4)
        .map(|state| {
            std::thread::spawn(move || {
                let mut backend = Backend::with_state(state).unwrap();
                let module = cached(&backend, EMPTY_MODULE);
                assert_eq!(*backend.state(), state);
                *backend.state_mut() += 10;
                assert_eq!(*backend.state(), state + 10);
                module
            })
        })
        .collect();
    let modules: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let backend = Backend::new().unwrap();
    let module = cached(&backend, EMPTY_MODULE);
    for other in modules {
        assert!(wasmtime::Module::same(&module, &other));
    }
}

#[test]
fn ordinary_compilation_bypasses_the_shared_cache() {
    let backend = Backend::new().unwrap();
    let first = pollster::block_on(backend.compiler().compile(EMPTY_MODULE)).unwrap();
    let retained = cached(&backend, EMPTY_MODULE);
    let second = pollster::block_on(backend.compiler().compile(EMPTY_MODULE)).unwrap();
    assert!(!wasmtime::Module::same(&first, &second));
    assert!(!wasmtime::Module::same(&first, &retained));
    assert!(!wasmtime::Module::same(&second, &retained));
}

#[test]
fn failed_compilation_does_not_prevent_later_modules_from_loading() {
    let backend = Backend::new().unwrap();
    assert!(pollster::block_on(backend.compiler().compile(b"invalid Wasm")).is_err());
    assert!(pollster::block_on(backend.compiler().compile(EMPTY_MODULE)).is_ok());
}
