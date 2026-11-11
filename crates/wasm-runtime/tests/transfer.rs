use futures_channel::oneshot;
use relocade_compiler::{abi::Function, Program, I32};
use relocade_wasm_runtime::{
    worker::{channel, Receiver, Sender, Thread, Transfer},
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
    let (sender, mut receiver) = channel::<CompiledUnit>().unwrap();
    let (installed, installation) = oneshot::channel();
    let worker = Thread::new();
    let metadata = Arc::new(Metadata {
        offset: 4,
        expected: 42,
    });
    let expected = metadata.clone();
    let (mut input, task) = worker
        .listen(
            move |mut input: Receiver<Sender<CompiledUnit>>| async move {
                let mut sender = input.recv().await.unwrap().unwrap();
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
                let memory = SharedMemory::new(1, 1).unwrap();
                memory.write(0, &[17]);
                sender
                    .send(CompiledUnit {
                        module,
                        memory: memory.clone(),
                        metadata,
                    })
                    .unwrap();
                // Delivery and installation happen while this compiler task stays alive.
                installation.await.unwrap();
                let mut bytes = [0; 8];
                memory.read(0, &mut bytes);
                assert_eq!(bytes, [17, 0, 0, 0, 42, 0, 0, 0]);
            },
        )
        .unwrap();
    input.send(sender).unwrap();
    let unit = receiver.recv().await.unwrap().unwrap();
    assert!(Arc::ptr_eq(&unit.metadata, &expected));
    let mut backend = Backend::new().unwrap();
    let instance = backend.instantiate(&unit.module, &[]).unwrap();
    let function = backend.function(&instance, VALUE).unwrap();
    let value = function.call(&mut backend, ());
    assert_eq!(value, unit.metadata.expected);
    let mut initial = [0];
    unit.memory.read(0, &mut initial);
    assert_eq!(initial, [17]);
    unit.memory
        .write(unit.metadata.offset, &value.to_le_bytes());
    installed.send(()).unwrap();
    task.await.unwrap();
    assert!(receiver.recv().await.is_none());
    drop(worker);
    let mut bytes = [0; 8];
    unit.memory.read(0, &mut bytes);
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
