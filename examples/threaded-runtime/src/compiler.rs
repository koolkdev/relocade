use crate::{backend, Backend};
use relocade_compiler::{abi::Function, Program, I64};
use relocade_runtime::wasm::{WasmBackend, WasmContext};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
};
use web_thread::Thread;

const VALUE: Function<fn() -> I64> = Function::new("test", "value");
const RUN: Function<fn() -> I64> = Function::new("test", "run");

// The library already transports JavaScript resources. Serde preserves the
// module handle so postMessage structured-clones compiled code, not Wasm bytes.
// Native modules satisfy the library's ordinary Send bound directly.
#[cfg_attr(target_arch = "wasm32", derive(serde::Serialize, serde::Deserialize))]
struct CompiledModule {
    #[cfg_attr(target_arch = "wasm32", serde(with = "serde_wasm_bindgen::preserve"))]
    module: <Backend as WasmBackend>::Module,
}

#[cfg(target_arch = "wasm32")]
impl web_thread::Post for CompiledModule {}

struct Request {
    input: Vec<u8>,
    observed_address: Arc<AtomicUsize>,
}

async fn compile(request: Request) -> Result<CompiledModule, String> {
    request
        .observed_address
        .store(request.input.as_ptr() as usize, Ordering::Release);
    let bytes = match request.input.as_slice() {
        [increment] => lower(*increment).map_err(|error| error.to_string())?,
        // Exercise a real backend compilation failure without killing the worker.
        _ => b"invalid Wasm".to_vec(),
    };
    let module = backend(())?
        .compile(&bytes)
        .await
        .map_err(|error| error.to_string())?;
    Ok(CompiledModule { module })
}

fn lower(increment: u8) -> Result<Vec<u8>, relocade_compiler::BuildError> {
    let mut program = Program::new();
    let value = program.import_function(VALUE);
    let run = program.function(RUN.signature(), |mut body| {
        let value = body.call::<I64>(value, &[])?;
        body.return_(value.add(u64::from(increment)))
    })?;
    RUN.export(&mut program, run)?;
    program.compile()
}

async fn execute(compilation: CompiledModule) -> Result<(i64, i64), String> {
    // Imports and mutable non-Send host state are created by the execution
    // owner. The compiler sends only reusable code.
    let mut backend = backend(Rc::new(Cell::new(40)))?;
    let value = backend.bind_host(VALUE, Box::new(|caller| caller.state().get()));
    let instance = backend
        .instantiate(&compilation.module, &[value])
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

pub(super) async fn check() -> Result<(), String> {
    let compiler = Thread::new();
    let execution = Thread::new();
    let observed_address = Arc::new(AtomicUsize::new(0));

    // Reuse the same compiler thread after a failed request. Rust input crosses
    // in the Send task closure; only the compiled JS module uses the Post result.
    for input in [vec![], vec![2]] {
        let expected_address = input.as_ptr() as usize;
        let request = Request {
            input,
            observed_address: Arc::clone(&observed_address),
        };
        let invalid = request.input.is_empty();
        let result = compiler
            .run((), move |()| compile(request))
            .await
            .map_err(|error| error.to_string())?;
        assert_eq!(observed_address.load(Ordering::Acquire), expected_address);
        if invalid {
            assert!(result.is_err());
            continue;
        }
        let compilation = result?;
        let values = execution
            .run_send(compilation, execute)
            .await
            .map_err(|error| error.to_string())??;
        assert_eq!(values, (42, 52));
    }
    Ok(())
}
