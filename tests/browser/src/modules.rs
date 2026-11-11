use relocade_compiler::{abi::Function, Program, I64};
use relocade_machine::{
    worker::{Receiver, Thread},
    Result,
};
use relocade_wasm_runtime::{FunctionRef, Module, WasmBackend, WasmCompiler, WasmRuntime};
use std::{cell::Cell, rc::Rc};

const VALUE: Function<fn() -> I64> = Function::new("test", "value");
const RUN: Function<fn() -> I64> = Function::new("test", "run");

fn lower() -> Result<Vec<u8>> {
    let mut program = Program::new();
    let value = program.import_function(VALUE);
    RUN.define(&mut program, move |mut body, ()| {
        let value = body.call::<I64>(value, &[])?;
        body.return_(value.add(2))
    })?;
    Ok(program.compile()?)
}

async fn execute(mut input: Receiver<Module>) -> std::result::Result<(i64, i64), String> {
    let module = input
        .recv()
        .await
        .unwrap()
        .map_err(|error| error.to_string())?;
    drop(input);
    let mut wasm = WasmRuntime::new().map_err(|error| error.to_string())?;
    let state = Rc::new(Cell::new(40));
    let captured_state = state.clone();
    let value =
        FunctionRef::new(&mut wasm.context(), move |_, ()| captured_state.get()).import(VALUE);
    let instance = wasm
        .instantiate(&module, &[value])
        .map_err(|error| error.to_string())?;
    let run = wasm
        .function(&instance, RUN)
        .map_err(|error| error.to_string())?;
    let first = wasm.call(&run, ());
    state.set(50);
    let second = wasm.call(&run, ());
    Ok((first, second))
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn compiled_modules() -> std::result::Result<(), String> {
    let module = WasmRuntime::new()
        .map_err(|error| error.to_string())?
        .compiler()
        .compile(&lower().map_err(|error| error.to_string())?)
        .await
        .map_err(|error| error.to_string())?;
    let execution = Thread::new();
    let (mut input, task) = execution
        .listen(execute)
        .map_err(|error| error.to_string())?;
    input.send(module).map_err(|error| error.to_string())?;
    let values = task.await.map_err(|error| error.to_string())??;
    assert_eq!(values, (42, 52));
    Ok(())
}
