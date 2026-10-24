use relocade_compiler::{abi, Program};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};
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
                let (mut memory, state) = caller.memory_and_state(&callback_memory);
                state.set(state.get() + 1);
                let value = memory.load::<u32>(0) + value;
                memory.store(0, value);
                value
            }),
        )];
        let instance = backend.instantiate(&module, &imports).await.unwrap();
        let run = backend.function(&instance, RUN).unwrap();
        {
            let (mut memory, state) = backend.memory_and_state(&memory);
            memory.store(0, 10_u32);
            state.set(20);
        }
        assert_eq!(run.call(&mut backend, (2,)), 12);
        assert_eq!(run.call(&mut backend, (3,)), 15);
        let (memory, state) = backend.memory_and_state(&memory);
        assert_eq!(memory.load::<u32>(0), 15);
        assert_eq!(state.get(), 22);
        assert_eq!(calls.get(), 22);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_borrow_live_memory_and_non_send_state() {
    pollster::block_on(check_callback_access());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_borrow_live_memory_and_non_send_state() {
    check_callback_access().await;
}
