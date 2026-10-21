//! Shared native and threaded-browser coverage of the production compiler worker.

use relocade_compiler::{abi::Function, Program, I64};
use relocade_runtime::{
    compiler_worker,
    wasm::{WasmBackend, WasmContext},
    worker::{StructuredClone, Thread},
    Error, Result,
};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};

#[cfg(not(target_arch = "wasm32"))]
use relocade_runtime_wasmtime::WasmtimeBackend as Backend;
#[cfg(target_arch = "wasm32")]
use relocade_runtime_web::WebBackend as Backend;

const VALUE: Function<fn() -> I64> = Function::new("test", "value");
const RUN: Function<fn() -> I64> = Function::new("test", "run");

thread_local! {
    static CALLER: Cell<bool> = const { Cell::new(false) };
}

fn backend() -> Result<Backend<Rc<Cell<i64>>>> {
    let state = Rc::new(Cell::new(40));
    #[cfg(not(target_arch = "wasm32"))]
    return Backend::with_state(state);
    #[cfg(target_arch = "wasm32")]
    Ok(Backend::with_state(state))
}

// Ordinary Rust input, with no message trait or serialization implementation.
struct Request {
    bytes: Vec<u8>,
    observed_address: Arc<AtomicUsize>,
}

fn lower(request: Request) -> Result<Vec<u8>> {
    assert!(!CALLER.get(), "lowering must run on the compiler thread");
    request
        .observed_address
        .store(request.bytes.as_ptr() as usize, Ordering::Release);
    let Some(&increment) = request.bytes.first() else {
        return Err(Error::wasm("lower request", "empty request"));
    };
    if increment == 255 {
        return Ok(b"invalid Wasm".to_vec());
    }
    let mut program = Program::new();
    let value = program.import_function(VALUE);
    let run = program.function(RUN.signature(), |mut body| {
        let value = body.call::<I64>(value, &[])?;
        body.return_(value.add(u64::from(increment)))
    })?;
    RUN.export(&mut program, run)?;
    Ok(program.compile()?)
}

async fn execute(
    StructuredClone(module): StructuredClone<<Backend<Rc<Cell<i64>>> as WasmBackend>::Module>,
) -> std::result::Result<(i64, i64), String> {
    let mut backend = backend().map_err(|error| error.to_string())?;
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
pub async fn run_checks() -> std::result::Result<(), String> {
    CALLER.set(true);
    let compiler = Thread::new();
    let execution = Thread::new();
    let observed_address = Arc::new(AtomicUsize::new(0));
    // Both lowering and backend errors affect only that job. The same thread
    // remains usable for the following valid request.
    for bytes in [vec![], vec![255], vec![2]] {
        let expected_address = bytes.as_ptr() as usize;
        let expected_error = match bytes.as_slice() {
            [] => Some("lower request"),
            [255] => Some("compile Wasm"),
            _ => None,
        };
        let request = Request {
            bytes,
            observed_address: Arc::clone(&observed_address),
        };
        let result = compiler
            .run((), move |()| {
                compiler_worker::compile(backend, move || lower(request))
            })
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(observed_address.load(Ordering::Acquire), expected_address);
        if let Some(operation) = expected_error {
            assert!(result.err().expect("request must fail").contains(operation));
            continue;
        }
        let values = execution
            .run_send(result?, execute)
            .await
            .map_err(|error| error.to_string())??;
        assert_eq!(values, (42, 52));
    }
    assert!(CALLER.get());
    // All tasks finish before their thread owners are dropped.
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn compilation_runs_on_a_worker_and_links_to_private_host_state() {
        pollster::block_on(super::run_checks()).unwrap();
    }
}
