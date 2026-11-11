#![cfg(not(target_arch = "wasm32"))]

use relocade_wasm_runtime::{WasmBackend, WasmCompiler, WasmRuntime};

const EMPTY_MODULE: &[u8] = b"\0asm\x01\0\0\0";

fn cached(runtime: &WasmRuntime, bytes: &'static [u8]) -> relocade_wasm_runtime::Module {
    let compiler = runtime.compiler();
    pollster::block_on(
        runtime
            .module_cache()
            .unwrap()
            .get_or_compile(bytes, |bytes| compiler.compile(bytes)),
    )
    .unwrap()
}

#[test]
fn concurrent_runtimes_share_cached_modules() {
    let threads: Vec<_> = (0..4)
        .map(|_| {
            std::thread::spawn(move || {
                let runtime = WasmRuntime::new().unwrap();
                cached(&runtime, EMPTY_MODULE)
            })
        })
        .collect();
    let modules: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    let runtime = WasmRuntime::new().unwrap();
    let module = cached(&runtime, EMPTY_MODULE);
    for other in modules {
        assert_eq!(module, other);
    }
}

#[test]
fn ordinary_compilation_bypasses_the_shared_cache() {
    let runtime = WasmRuntime::new().unwrap();
    let first = pollster::block_on(runtime.compiler().compile(EMPTY_MODULE)).unwrap();
    let retained = cached(&runtime, EMPTY_MODULE);
    let second = pollster::block_on(runtime.compiler().compile(EMPTY_MODULE)).unwrap();
    assert_ne!(first, second);
    assert_ne!(first, retained);
    assert_ne!(second, retained);
}

#[test]
fn failed_compilation_does_not_prevent_later_modules_from_loading() {
    let runtime = WasmRuntime::new().unwrap();
    assert!(pollster::block_on(runtime.compiler().compile(b"invalid Wasm")).is_err());
    assert!(pollster::block_on(runtime.compiler().compile(EMPTY_MODULE)).is_ok());
}
