use relocade_compiler::{abi, Program};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext, WasmMemoryAccess};
use std::{cell::Cell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_callback_access() {
    const HOST: abi::Function<fn(u32) -> u32> = abi::Function::new("context", "host");
    const RUN: abi::Function<fn(u32) -> u32> = abi::Function::new("context", "run");
    for shared in [false, true] {
        let calls = Rc::new(Cell::new(0_u32));
        let mut backend = Backend::with_state(Rc::clone(&calls)).unwrap();
        let memory = backend
            .create_memory(abi::Memory {
                shared,
                maximum: Some(1),
                ..abi::Memory::new("context", "memory", 1)
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
        let callback_memory = memory.clone();
        let imports = [backend.bind_host(
            HOST,
            Box::new(move |mut caller, value| {
                caller.state().set(caller.state().get() + 1);
                let mut memory = caller.memory(&callback_memory);
                let value = memory.load::<u32>(0) + value;
                memory.store(0, value);
                value
            }),
        )];
        let instance = backend.instantiate(&module, &imports).await.unwrap();
        let run = backend.function(&instance, RUN).unwrap();
        backend.memory(&memory).store(0, 10_u32);
        backend.state().set(20);
        assert_eq!(run.call(&mut backend, (2,)), 12);
        assert_eq!(run.call(&mut backend, (3,)), 15);
        assert_eq!(backend.memory(&memory).load::<u32>(0), 15);
        assert_eq!(backend.state().get(), 22);
        assert_eq!(calls.get(), 22);

        // Engine handles can survive teardown without retaining backend state.
        drop(backend);
        assert_eq!(Rc::strong_count(&calls), 1);
        drop((imports, instance, run));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_borrow_memory_and_state_for_the_backend_lifetime() {
    pollster::block_on(check_callback_access());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_borrow_memory_and_state_for_the_backend_lifetime() {
    check_callback_access().await;
}
