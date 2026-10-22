use relocade_compiler::{abi::Function, Program, I64};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_export_results() {
    const RESULTS: [(&str, i64); 4] = [
        ("zero", 0),
        ("minimum", i64::MIN),
        ("maximum", i64::MAX),
        ("wide", -0x20_0000_0000_0001),
    ];
    let mut program = Program::new();
    for (name, value) in RESULTS {
        let export = Function::<fn() -> I64>::new("test", name);
        let function = program
            .function(export.signature(), |body| body.return_(value as u64))
            .unwrap();
        export.export(&mut program, function).unwrap();
    }
    let trap = Function::<fn() -> I64>::new("test", "trap");
    let function = program
        .function(trap.signature(), |body| body.trap())
        .unwrap();
    trap.export(&mut program, function).unwrap();

    let mut backend = Backend::new().unwrap();
    let module = backend
        .compiler()
        .compile(&program.compile().unwrap())
        .await
        .unwrap();
    let instance = backend.instantiate(&module, &[]).await.unwrap();
    let function = backend.function(&instance, trap).unwrap();
    assert!(backend.call(&function).is_err());
    for (name, expected) in RESULTS {
        let function = backend
            .function(&instance, Function::new("test", name))
            .unwrap();
        assert_eq!(backend.call(&function).unwrap(), expected);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn exports_preserve_i64_results_and_report_traps() {
    pollster::block_on(check_export_results());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn exports_preserve_i64_results_and_report_traps() {
    check_export_results().await;
}
