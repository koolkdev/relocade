use crate::{backend, Backend};
use relocade_compiler::{abi::Function, Program, I64};
use relocade_runtime::{
    compiler_worker,
    wasm::{WasmBackend, WasmContext},
    worker::{Message, Thread},
    Result,
};
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
    let mut backend = backend(Rc::new(Cell::new(40))).map_err(|error| error.to_string())?;
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
    let compiler = Thread::new();
    let execution = Thread::new();
    let module = compiler
        .run((), |()| compiler_worker::compile(|| backend(()), lower))
        .await
        .map_err(|error| error.to_string())??;
    let values = execution
        .run_send(module, execute)
        .await
        .map_err(|error| error.to_string())??;
    assert_eq!(values, (42, 52));
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[test]
fn compiler_errors_leave_the_worker_usable() {
    pollster::block_on(async {
        let compiler = Thread::new();
        for (bytes, operation) in [
            (None, "lower request"),
            (Some(b"invalid Wasm"), "compile Wasm"),
        ] {
            let result = compiler
                .run((), move |()| {
                    compiler_worker::compile(
                        || backend(()),
                        move || match bytes {
                            Some(bytes) => Ok(bytes.to_vec()),
                            None => Err(relocade_runtime::Error::wasm("lower request", "failed")),
                        },
                    )
                })
                .await
                .unwrap();
            assert!(result.err().expect("request must fail").contains(operation));
        }
        assert!(compiler
            .run((), |()| compiler_worker::compile(|| backend(()), lower))
            .await
            .unwrap()
            .is_ok());
    });
}
