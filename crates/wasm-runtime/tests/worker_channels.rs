use futures_channel::mpsc;
use futures_util::{FutureExt, StreamExt};
use relocade_compiler::{abi::Function, Program, I32};
use relocade_wasm_runtime::{
    worker::{channel, Thread, ThreadExt},
    Backend, Module, WasmBackend, WasmCompiler,
};

const VALUE: Function<fn() -> I32> = Function::new("test", "value");

async fn check_channels() {
    let (modules, incoming) = channel::<Module>().unwrap();
    let (requests, mut jobs) = mpsc::unbounded();
    let compilation = Thread::new();
    let compile = compilation.run_resource(modules, move |modules| async move {
        let modules = modules.bind();
        let compiler = Backend::new().unwrap().compiler();
        while let Some(value) = jobs.next().await {
            let mut program = Program::new();
            VALUE
                .define(&mut program, move |body, ()| body.return_(value))
                .unwrap();
            let module = compiler.compile(&program.compile().unwrap()).await.unwrap();
            modules.send(module).unwrap();
        }
        // Dropping the port closes delivery after both modules, while the worker
        // remains alive until its owner awaits this task.
    });
    let execution = Thread::new();
    let execute = execution.run_resource(incoming, move |incoming| async move {
        let mut incoming = incoming.bind();
        // A canceled receive waiter must neither consume nor lose a message.
        assert!(incoming.recv().now_or_never().is_none());
        requests.unbounded_send(42_i32).unwrap();
        requests.unbounded_send(73_i32).unwrap();
        drop(requests);
        let mut backend = Backend::new().unwrap();
        let mut values = Vec::new();
        while let Some(module) = incoming.recv().await {
            let instance = backend.instantiate(&module.unwrap(), &[]).await.unwrap();
            let value = backend.function(&instance, VALUE).unwrap();
            values.push(backend.call(&value, ()));
        }
        values
    });
    assert_eq!(execute.await.unwrap(), [42, 73]);
    compile.await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn persistent_workers_exchange_modules_and_close_in_order() {
    pollster::block_on(check_channels());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn persistent_workers_exchange_modules_and_close_in_order() {
    check_channels().await;
}
