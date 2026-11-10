use relocade_compiler::{abi::Function, Program, I32};
use relocade_wasm_runtime::{
    worker::{Thread, ThreadExt},
    Backend, SharedMemory, WasmBackend, WasmCompiler, WasmSharedMemory,
};

const VALUE: Function<fn() -> I32> = Function::new("test", "value");

async fn engine_handles() {
    let mut program = Program::new();
    VALUE
        .define(&mut program, |body, ()| body.return_(42))
        .unwrap();
    let module = Backend::new()
        .unwrap()
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let worker = Thread::new();
    let value = worker
        .run_resource(module, |module| async move {
            let mut backend = Backend::new().unwrap();
            let instance = backend.instantiate(&module, &[]).await.unwrap();
            let function = backend.function(&instance, VALUE).unwrap();
            function.call(&mut backend, ())
        })
        .await
        .unwrap();
    assert_eq!(value, 42);

    let memory = SharedMemory::new(1, 1).unwrap();
    memory.write(0, &[17]);
    worker
        .run_resource(memory.clone(), move |memory| async move {
            let mut initial = [0];
            memory.read(0, &mut initial);
            assert_eq!(initial, [17]);
            memory.write(0, &value.to_le_bytes());
        })
        .await
        .unwrap();
    drop(worker);
    let mut bytes = [0; 4];
    memory.read(0, &mut bytes);
    assert_eq!(bytes, [42, 0, 0, 0]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn modules_and_memory_work_on_another_worker() {
    pollster::block_on(engine_handles());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn modules_and_memory_work_on_another_worker() {
    engine_handles().await;
}
