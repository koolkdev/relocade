use relocade_compiler::{abi::Function, Program, I64};
use relocade_wasm_runtime::{FunctionRef, WasmBackend, WasmCompiler, WasmRuntime};
use std::{cell::Cell, rc::Rc};

const VALUE: Function<fn() -> I64> = Function::new("test", "value");
const RUN: Function<fn() -> I64> = Function::new("test", "run");

async fn check_independent_compiler() {
    let runtime = WasmRuntime::new().unwrap();
    let compiler = runtime.compiler();

    let mut program = Program::new();
    let value = program.import_function(VALUE);
    let run = program.function::<fn() -> I64>(move |mut body, ()| {
        let value = body.call::<I64>(value, &[])?;
        body.return_(value)
    });
    RUN.export(&mut program, run).unwrap();
    let bytes = program.compile().unwrap();
    let compilation = compiler.compile(&bytes);

    // Compilation remains usable after the originating execution owner is gone.
    drop(runtime);
    let module = compilation.await.unwrap();

    let mut runtime = WasmRuntime::new().unwrap();
    let state = Rc::new(Cell::new(42_i64));
    let captured_state = state.clone();
    let value =
        FunctionRef::new(&mut runtime.context(), move |_, ()| captured_state.get()).import(VALUE);
    let instance = runtime.instantiate(&module, &[value]).unwrap();
    let run = runtime.function(&instance, RUN).unwrap();
    assert_eq!(runtime.call(&run, ()), 42);
    state.set(52);
    assert_eq!(runtime.call(&run, ()), 52);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compilation_outlives_its_runtime_and_binds_to_new_callbacks() {
    pollster::block_on(check_independent_compiler());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compilation_outlives_its_runtime_and_binds_to_new_callbacks() {
    check_independent_compiler().await;
}
