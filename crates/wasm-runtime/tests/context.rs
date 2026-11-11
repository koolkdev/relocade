use relocade_compiler::{abi, Program};
use relocade_wasm_runtime::{
    FunctionRef, WasmBackend, WasmCompiler, WasmMemoryAccess, WasmRuntime,
};
use std::{cell::Cell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_callback_access() {
    const HOST: abi::Function<fn(u32) -> u32> = abi::Function::new("context", "host");
    const RUN: abi::Function<fn(u32) -> u32> = abi::Function::new("context", "run");
    for shared in [false, true] {
        let calls = Rc::new(Cell::new(0_u32));
        let mut runtime = WasmRuntime::new().unwrap();
        let memory = runtime
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
        let module = runtime
            .compiler()
            .compile(&program.compile().unwrap())
            .await
            .unwrap();
        let callback_memory = memory.clone();
        let callback_calls = Rc::downgrade(&calls);
        let imports = [
            FunctionRef::new(&mut runtime.context(), move |mut caller, (value,)| {
                let calls = callback_calls.upgrade().unwrap();
                calls.set(calls.get() + 1);
                let mut memory = caller.memory(&callback_memory);
                let value = memory.load::<u32>(0) + value;
                memory.store(0, value);
                value
            })
            .import(HOST),
        ];
        let instance = runtime.instantiate(&module, &imports).unwrap();
        let run = runtime.function(&instance, RUN).unwrap();
        runtime.memory(&memory).store(0, 10_u32);
        calls.set(20);
        assert_eq!(run.call(&mut runtime, (2,)), 12);
        assert_eq!(run.call(&mut runtime, (3,)), 15);
        assert_eq!(runtime.memory(&memory).load::<u32>(0), 15);
        assert_eq!(calls.get(), 22);

        // Weak captures let the application owner release state independently.
        let lifetime = Rc::downgrade(&calls);
        drop(calls);
        assert!(lifetime.upgrade().is_none());
        drop(runtime);
        drop((imports, instance, run));
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn callbacks_access_captured_state_and_scoped_memory() {
    pollster::block_on(check_callback_access());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn callbacks_access_captured_state_and_scoped_memory() {
    check_callback_access().await;
}
