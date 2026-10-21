use relocade_compiler::{abi::Function, Program, I64};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};
use std::{cell::Cell, rc::Rc};

const VALUE: Function<fn() -> I64> = Function::new("test", "value");
const RUN: Function<fn() -> I64> = Function::new("test", "run");

async fn check_independent_compiler() {
    let state = Rc::new(Cell::new(100_i64));
    let released = Rc::downgrade(&state);
    let backend = Backend::with_state(state).unwrap();
    let compiler = backend.compiler();

    let mut program = Program::new();
    let value = program.import_function(VALUE);
    let run = program
        .function(RUN.signature(), |mut body| {
            let value = body.call::<I64>(value, &[])?;
            body.return_(value)
        })
        .unwrap();
    RUN.export(&mut program, run).unwrap();
    let bytes = program.compile().unwrap();
    let compilation = compiler.compile(&bytes);

    // Neither the compiler nor its future retains the originating store's state.
    drop(backend);
    assert!(released.upgrade().is_none());
    let module = compilation.await.unwrap();

    let mut backend = Backend::with_state(Rc::new(Cell::new(42_i64))).unwrap();
    let value = backend.bind_host(VALUE, Box::new(|caller| caller.state().get()));
    let instance = backend.instantiate(&module, &[value]).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&run).unwrap(), 42);
    backend.state().set(52);
    assert_eq!(backend.call(&run).unwrap(), 52);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn compilation_outlives_its_backend_and_binds_to_new_host_state() {
    pollster::block_on(check_independent_compiler());
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn compilation_outlives_its_backend_and_binds_to_new_host_state() {
    check_independent_compiler().await;
}
