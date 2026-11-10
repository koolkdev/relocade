use futures_channel::mpsc;
use futures_util::{FutureExt, StreamExt};
use relocade_compiler::{abi::Function, Program, I32};
use relocade_wasm_runtime::{
    worker::{Sender, Worker, WorkerResource},
    Backend, Module, SharedMemory, WasmBackend, WasmCompiler, WasmSharedMemory,
};

const VALUE: Function<fn() -> I32> = Function::new("test", "value");

#[derive(WorkerResource)]
struct CompiledValue {
    #[worker(resource)]
    module: Module,
    #[worker(resource)]
    memory: SharedMemory,
    expected: Box<(u32, i32)>,
}

#[derive(WorkerResource)]
struct Compilation {
    #[worker(resource)]
    output: Sender<CompiledValue>,
    #[worker(resource)]
    memory: SharedMemory,
}

async fn check_channels() {
    let execution = Worker::new();
    let (modules, mut incoming) = execution.channel::<CompiledValue>().await.unwrap();
    let (requests, mut jobs) = mpsc::unbounded();
    let memory = SharedMemory::new(1, 1).unwrap();
    let compilation = Worker::new();
    let input = Compilation {
        output: modules,
        memory: memory.clone(),
    };
    let (mut inputs, mut pending) = compilation.channel::<Compilation>().await.unwrap();
    let compile = compilation.run(move || async move {
        let mut input = pending.recv().await.unwrap().unwrap();
        let compiler = Backend::new().unwrap().compiler();
        while let Some((offset, value)) = jobs.next().await {
            let mut program = Program::new();
            VALUE
                .define(&mut program, move |body, ()| body.return_(value))
                .unwrap();
            let module = compiler.compile(&program.compile().unwrap()).await.unwrap();
            input
                .output
                .send(CompiledValue {
                    module,
                    memory: input.memory.clone(),
                    expected: Box::new((offset, value)),
                })
                .unwrap();
        }
        // Dropping the sender closes delivery after both modules, while the worker
        // remains alive until its owner awaits this task.
    });
    inputs.send(input).unwrap();
    let execute = execution.run(move || async move {
        // A canceled receive waiter must neither consume nor lose a message.
        assert!(incoming.recv().now_or_never().is_none());
        requests.unbounded_send((0, 42_i32)).unwrap();
        requests.unbounded_send((4, 73_i32)).unwrap();
        drop(requests);
        let mut backend = Backend::new().unwrap();
        let mut values = Vec::new();
        while let Some(compiled) = incoming.recv().await {
            let compiled = compiled.unwrap();
            let instance = backend.instantiate(&compiled.module, &[]).await.unwrap();
            let value = backend.function(&instance, VALUE).unwrap();
            let value = value.call(&mut backend, ());
            let (offset, expected) = *compiled.expected;
            assert_eq!(value, expected);
            compiled.memory.write(offset, &value.to_le_bytes());
            values.push(value);
        }
        values
    });
    compile.await.unwrap();
    drop(compilation);
    assert_eq!(execute.await.unwrap(), [42, 73]);
    let mut bytes = [0; 8];
    memory.read(0, &mut bytes);
    assert_eq!(bytes, [42, 0, 0, 0, 73, 0, 0, 0]);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistent_workers_exchange_modules_and_shared_memory() {
    pollster::block_on(check_channels());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn persistent_workers_exchange_modules_and_shared_memory() {
    check_channels().await;
}
