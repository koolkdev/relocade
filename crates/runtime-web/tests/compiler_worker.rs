use relocade_compiler::{abi::Function, Program, I64};
use relocade_runtime::{
    compiler_worker::{self, CompilerWorker, LoweredModule},
    wasm::{WasmBackend, WasmContext},
    Result,
};
use std::{cell::Cell, rc::Rc};

const VALUE: Function<fn() -> I64> = Function::new("test", "value");
const RUN: Function<fn() -> I64> = Function::new("test", "run");

fn lower(bytes: &[u8]) -> Result<Option<LoweredModule>> {
    let Some(&increment) = bytes.first() else {
        return Ok(None);
    };
    if increment == 255 {
        return Ok(Some(LoweredModule {
            wasm: b"invalid Wasm".to_vec(),
            metadata: vec![],
        }));
    }
    let mut program = Program::new();
    let value = program.import_function(VALUE);
    let run = program.function(RUN.signature(), |mut body| {
        let value = body.call::<I64>(value, &[])?;
        body.return_(value.add(u64::from(increment)))
    })?;
    RUN.export(&mut program, run)?;
    Ok(Some(LoweredModule {
        wasm: program.compile()?,
        metadata: bytes.to_vec(),
    }))
}

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen]
pub async fn compile_test_module(
    _: wasm_bindgen::JsValue,
    endpoint: relocade_worker::web::Endpoint,
    _: wasm_bindgen::JsValue,
) -> std::result::Result<(), wasm_bindgen::JsValue> {
    compiler_worker::run(
        endpoint.into(),
        relocade_runtime_web::WebBackend::new(),
        lower,
    )
    .await
    .map_err(|error| error.to_string().into())
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(inline_js = "
export function test_url(path) { return new URL(path, location.href).href; }
")]
extern "C" {
    fn test_url(path: &str) -> String;
}

#[cfg(target_arch = "wasm32")]
fn compiler() -> CompilerWorker<js_sys::WebAssembly::Module> {
    CompilerWorker::from_worker(
        relocade_worker::web::Worker::spawn(
            &test_url("/wasm-bindgen-test"),
            &test_url("/wasm-bindgen-test_bg.wasm"),
            "compile_test_module",
            &wasm_bindgen::JsValue::NULL,
        )
        .unwrap(),
        2,
    )
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn worker_returns_code_for_private_non_send_host_state() {
    let mut compiler = compiler();
    let compilation = compiler.submit(vec![2]).unwrap().unwrap();
    let unsupported = compiler.submit(vec![]).unwrap().unwrap();
    assert!(compiler.submit(vec![3]).unwrap().is_none());
    drop(compiler);
    let compilation = compilation.await.unwrap().unwrap();
    assert_eq!(compilation.metadata, [2]);
    assert!(unsupported.await.unwrap().is_none());

    let mut backend = relocade_runtime_web::WebBackend::with_state(Rc::new(Cell::new(40)));
    check_module(&mut backend, &compilation.module).await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn worker_returns_code_for_private_non_send_host_state() {
    pollster::block_on(async {
        let caller_thread = std::thread::current().id();
        let mut compiler = CompilerWorker::spawn("test-compiler", 2, move |endpoint| async move {
            let backend = relocade_runtime_wasmtime::WasmtimeBackend::new()
                .map_err(|error| relocade_worker::Error::Failed(error.to_string()))?;
            compiler_worker::run(endpoint, backend, move |request| {
                assert_ne!(std::thread::current().id(), caller_thread);
                lower(request)
            })
            .await
        })
        .unwrap();
        let compilation = compiler.submit(vec![2]).unwrap().unwrap();
        fn is_send<T: Send>(_: &T) {}
        is_send(&compilation);
        let unsupported = compiler.submit(vec![]).unwrap().unwrap();
        let compilation = compilation.await.unwrap().unwrap();
        assert_eq!(compilation.metadata, [2]);
        assert!(unsupported.await.unwrap().is_none());
        assert!(compiler.submit(vec![255]).unwrap().unwrap().await.is_err());
        let recovered = compiler.submit(vec![3]).unwrap().unwrap();
        drop(compiler);
        assert_eq!(recovered.await.unwrap().unwrap().metadata, [3]);
        let mut backend =
            relocade_runtime_wasmtime::WasmtimeBackend::with_state(Rc::new(Cell::new(40))).unwrap();
        check_module(&mut backend, &compilation.module).await;
    });
}

async fn check_module<
    B: WasmBackend<State = Rc<Cell<i64>>> + relocade_runtime::wasm::HostFunction<fn() -> I64>,
>(
    backend: &mut B,
    module: &B::Module,
) {
    let import = backend.bind_host(VALUE, Box::new(|caller| caller.state().get()));
    let instance = backend.instantiate(module, &[import]).await.unwrap();
    let run = backend.function(&instance, RUN).unwrap();
    assert_eq!(backend.call(&run).unwrap(), 42);
    backend.state().set(50);
    assert_eq!(backend.call(&run).unwrap(), 52);
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn worker_reports_compile_and_startup_errors() {
    let mut compiler = compiler();
    assert!(compiler.submit(vec![255]).unwrap().unwrap().await.is_err());
    assert!(compiler
        .submit(vec![1])
        .unwrap()
        .unwrap()
        .await
        .unwrap()
        .is_some());

    let mut missing = CompilerWorker::<js_sys::WebAssembly::Module>::from_worker(
        relocade_worker::web::Worker::spawn(
            &test_url("/missing-compiler.js"),
            &test_url("/missing-compiler.wasm"),
            "compile_test_module",
            &wasm_bindgen::JsValue::NULL,
        )
        .unwrap(),
        2,
    );
    let first = missing.submit(vec![1]).unwrap().unwrap();
    let second = missing.submit(vec![2]).unwrap().unwrap();
    assert!(missing.ready().await.is_err());
    assert!(first.await.is_err());
    assert!(second.await.is_err());
    assert!(missing.stopped().await.is_err());
}
