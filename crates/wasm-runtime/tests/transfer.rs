use relocade_compiler::{abi::Function, Program, I32};
use relocade_wasm_runtime::{
    worker::{Thread, ThreadExt, Transfer},
    Backend, Module, SharedMemory, WasmBackend, WasmCompiler, WasmSharedMemory,
};
use std::sync::Arc;

const VALUE: Function<fn() -> I32> = Function::new("test", "value");

struct Metadata {
    offset: u32,
    expected: i32,
}

#[derive(Transfer)]
struct CompiledUnit {
    module: Module,
    memory: SharedMemory,
    metadata: Arc<Metadata>,
}

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
    let memory = SharedMemory::new(1, 1).unwrap();
    memory.write(0, &[17]);
    let metadata = Arc::new(Metadata {
        offset: 4,
        expected: 42,
    });
    let expected = metadata.clone();
    let value = worker
        .run_resource(
            CompiledUnit {
                module,
                memory: memory.clone(),
                metadata,
            },
            move |unit| async move {
                assert!(Arc::ptr_eq(&unit.metadata, &expected));
                let mut backend = Backend::new().unwrap();
                let instance = backend.instantiate(&unit.module, &[]).await.unwrap();
                let function = backend.function(&instance, VALUE).unwrap();
                let value = function.call(&mut backend, ());
                assert_eq!(value, unit.metadata.expected);
                let mut initial = [0];
                unit.memory.read(0, &mut initial);
                assert_eq!(initial, [17]);
                unit.memory
                    .write(unit.metadata.offset, &value.to_le_bytes());
                value
            },
        )
        .await
        .unwrap();
    assert_eq!(value, 42);

    drop(worker);
    let mut bytes = [0; 8];
    memory.read(0, &mut bytes);
    assert_eq!(bytes, [17, 0, 0, 0, 42, 0, 0, 0]);
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
