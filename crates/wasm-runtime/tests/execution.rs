use relocade_compiler::{abi, Program};
use relocade_wasm_runtime::{
    Backend, Function, Memory, WasmBackend, WasmCallback, WasmCompiler, WasmContext,
};
use std::{cell::Cell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

const HOST: abi::Function<fn(u32) -> u32> = abi::Function::new("execution", "host");
const RUN: abi::Function<fn(u32) -> u32> = abi::Function::new("execution", "run");

struct Resources {
    memory: Memory,
    increment: u32,
}

#[derive(Default)]
struct State {
    resource_address: usize,
    calls: Rc<Cell<u32>>,
}

struct Fixture {
    backend: Backend<State>,
    run: Function<Backend<State>, fn(u32) -> u32>,
    resources: Resources,
}

impl Fixture {
    async fn new(shared: bool) -> Self {
        let mut backend = Backend::with_state(State::default()).unwrap();
        let memory = backend
            .create_memory(abi::Memory {
                shared,
                maximum: Some(1),
                ..abi::Memory::new("execution", "memory", 1)
            })
            .unwrap();
        let mut program = Program::new();
        let host = HOST.import(&mut program);
        RUN.define(&mut program, move |body, (value,)| {
            host.tail_call(body, value)
        })
        .unwrap();
        let module = backend
            .compiler()
            .compile(&program.compile().unwrap())
            .await
            .unwrap();
        let imports = [backend.bind_host(
            HOST,
            Box::new(|mut caller, value| {
                caller.with_execution_data::<Resources, _>(|caller, resources| {
                    assert_ne!(value, 0, "handler failed");
                    let (mut memory, mut state) = caller.memory_and_state(&resources.memory);
                    state.resource_address = std::ptr::from_ref(resources) as usize;
                    state.calls.set(state.calls.get() + 1);
                    let value = memory.load::<u32>(0) + value + resources.increment;
                    memory.store(0, value);
                    value
                })
            }),
        )];
        let instance = backend.instantiate(&module, &imports).await.unwrap();
        let run = backend.function(&instance, RUN).unwrap();
        Self {
            backend,
            run,
            resources: Resources {
                memory,
                increment: 3,
            },
        }
    }
}

async fn check_execution_resources() {
    for shared in [false, true] {
        let Fixture {
            mut backend,
            run,
            mut resources,
        } = Fixture::new(shared).await;
        // Stopped access owns the browser store guard while lending live memory.
        {
            let (mut memory, state) = backend.memory_and_state(&resources.memory);
            memory.store(0, 10_u32);
            state.calls.set(20);
        }
        let mut execution = backend.execution();
        assert_eq!(run.call_with(&mut execution, &resources, (2,)), 15);
        assert_eq!(
            execution.state().resource_address,
            std::ptr::from_ref(&resources) as usize,
        );
        // The hook borrows the current owner; it retains no snapshot or copy.
        resources.increment = 7;
        assert_eq!(run.call_with(&mut execution, &resources, (3,)), 25);
        assert_eq!(execution.state().calls.get(), 22);
        let (memory, state) = execution.memory_and_state(&resources.memory);
        assert_eq!(memory.load::<u32>(0), 25);
        assert_eq!(
            state.resource_address,
            std::ptr::from_ref(&resources) as usize
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_borrow_the_active_resources_and_non_send_state() {
    pollster::block_on(check_execution_resources());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_borrow_the_active_resources_and_non_send_state() {
    check_execution_resources().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn returning_or_unwinding_removes_the_resource_loan() {
    use std::panic::{catch_unwind, AssertUnwindSafe};

    for unwind in [false, true] {
        let Fixture {
            mut backend,
            run,
            resources,
        } = pollster::block_on(Fixture::new(false));
        let mut execution = backend.execution();
        let call = catch_unwind(AssertUnwindSafe(|| {
            run.call_with(&mut execution, &resources, (u32::from(!unwind),))
        }));
        assert_eq!(call.is_err(), unwind);
        drop(resources);
        // Inspect through the public callback API after the resources have gone.
        // A stale loan would dereference freed resources here.
        let callback = catch_unwind(AssertUnwindSafe(|| {
            execution.with_execution_data::<Resources, _>(|_, _| ())
        }));
        let message = callback.unwrap_err();
        let message = message
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| message.downcast_ref::<&str>().copied());
        assert_eq!(
            message,
            Some("callback requires active execution resources"),
        );
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
#[should_panic(expected = "callback execution resource type mismatch")]
fn callbacks_cannot_borrow_a_different_resource_type() {
    let Fixture {
        mut backend, run, ..
    } = pollster::block_on(Fixture::new(false));
    run.call_with(&mut backend.execution(), &0_u8, (1,));
}
