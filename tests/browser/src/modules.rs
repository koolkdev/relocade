use relocade_compiler::{abi::Function, Program, I64};
use relocade_machine::{
    worker::{Message, Thread},
    Result,
};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};
use std::{cell::Cell, rc::Rc};

const VALUE: Function<fn() -> I64> = Function::new("test", "value");
const RUN: Function<fn() -> I64> = Function::new("test", "run");

fn lower() -> Result<Vec<u8>> {
    let mut program = Program::new();
    let value = program.import_function(VALUE);
    let run = program.function(RUN.signature(), |mut body| {
        let value = body.call::<I64>(value, &[])?;
        body.return_(value.add(2))
    })?;
    RUN.export(&mut program, run)?;
    Ok(program.compile()?)
}

async fn execute(
    Message(module): Message<<Backend<()> as WasmBackend>::Module>,
) -> std::result::Result<(i64, i64), String> {
    let mut backend =
        Backend::with_state(Rc::new(Cell::new(40))).map_err(|error| error.to_string())?;
    let value = backend.bind_host(VALUE, Box::new(|caller| caller.state().get()));
    let instance = backend
        .instantiate(&module, &[value])
        .await
        .map_err(|error| error.to_string())?;
    let run = backend
        .function(&instance, RUN)
        .map_err(|error| error.to_string())?;
    let first = backend.call(&run).map_err(|error| error.to_string())?;
    backend.state().set(50);
    let second = backend.call(&run).map_err(|error| error.to_string())?;
    Ok((first, second))
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen::prelude::wasm_bindgen)]
pub async fn compiled_modules() -> std::result::Result<(), String> {
    let module = Backend::new()
        .map_err(|error| error.to_string())?
        .compiler()
        .compile(&lower().map_err(|error| error.to_string())?)
        .await
        .map_err(|error| error.to_string())?;
    let execution = Thread::new();
    let values = execution
        .run_send(Message(module), execute)
        .await
        .map_err(|error| error.to_string())??;
    assert_eq!(values, (42, 52));
    Ok(())
}
