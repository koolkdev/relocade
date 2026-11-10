use futures_channel::mpsc;
use futures_util::{
    future::{select, Either},
    FutureExt, StreamExt,
};
use relocade_compiler::{abi::Function, Program, I32};
use relocade_wasm_runtime::{
    worker::{channel, wake_connection, Executor, Thread, ThreadExt, WakeTarget, WorkerResource},
    Backend, Module, WasmBackend, WasmCompiler,
};
use std::{
    cell::Cell,
    future::{pending, poll_fn},
    pin::pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    task::Poll,
};

const VALUE: Function<fn() -> I32> = Function::new("test", "value");

#[derive(WorkerResource)]
struct CompiledValue {
    #[worker(post)]
    module: Module,
    expected: Box<i32>,
}

#[derive(Clone)]
struct Signal(Arc<AtomicBool>);
impl WakeTarget for Signal {
    fn wake(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    fn acknowledge(&self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

async fn check_channels() {
    let (modules, incoming) = channel::<CompiledValue>().unwrap();
    let (requests, mut jobs) = mpsc::unbounded();
    let compilation = Thread::new();
    let compile = compilation.run_resource(modules, move |mut modules| async move {
        let compiler = Backend::new().unwrap().compiler();
        while let Some(value) = jobs.next().await {
            let mut program = Program::new();
            VALUE
                .define(&mut program, move |body, ()| body.return_(value))
                .unwrap();
            let module = compiler.compile(&program.compile().unwrap()).await.unwrap();
            modules
                .send(CompiledValue {
                    module,
                    expected: Box::new(value),
                })
                .unwrap();
        }
        // Dropping the sender closes delivery after both modules, while the worker
        // remains alive until its owner awaits this task.
    });
    let execution = Thread::new();
    let signal = Signal(Arc::default());
    let (_host, wake) = wake_connection(signal.clone());
    let execute = execution.run_resource(incoming, move |incoming| async move {
        let executor = Executor::new(wake.bind(signal));
        let mut incoming = incoming.bind_on(&executor);
        let polls = Cell::new(0);
        let receive = async {
            // A canceled receive waiter must neither consume nor lose a message.
            assert!(incoming.recv().now_or_never().is_none());
            requests.unbounded_send(42_i32).unwrap();
            requests.unbounded_send(73_i32).unwrap();
            drop(requests);
            let mut backend = Backend::new().unwrap();
            let mut values = Vec::new();
            while let Some(compiled) = incoming.recv().await {
                let compiled = compiled.unwrap();
                let instance = backend.instantiate(&compiled.module, &[]).await.unwrap();
                let value = backend.function(&instance, VALUE).unwrap();
                let value = backend.call(&value, ());
                assert_eq!(value, *compiled.expected);
                values.push(value);
            }
            values
        };
        let busy = poll_fn(|cx| {
            polls.set(polls.get() + 1);
            cx.waker().wake_by_ref();
            Poll::<()>::Pending
        });
        let values = executor
            .run_until(pending::<()>(), async {
                match select(pin!(busy), pin!(receive)).await {
                    Either::Right((values, _)) => values,
                    Either::Left(_) => unreachable!(),
                }
            })
            .await
            .unwrap();
        assert!(polls.get() > 0);
        values
    });
    assert_eq!(execute.await.unwrap(), [42, 73]);
    compile.await.unwrap();
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn busy_workers_exchange_modules_and_close_in_order() {
    pollster::block_on(check_channels());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn busy_workers_exchange_modules_and_close_in_order() {
    check_channels().await;
}
