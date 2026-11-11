use relocade_compiler::{abi, Program, I32};
use relocade_wasm_runtime::{FunctionRef, WasmBackend, WasmCompiler, WasmRuntime};
use std::{cell::RefCell, rc::Rc};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[path = "tables/callbacks.rs"]
mod callbacks;

async fn check_table_defaults() {
    type Read = fn(u16) -> u32;
    const READ: abi::Table<Read> = {
        let mut table = abi::Table::new("test", "read", 2);
        table.maximum = Some(4);
        table
    };
    const RUN: abi::Function<fn(u32, u16) -> u32> = abi::Function::new("test", "run");
    const REPLACEMENT: abi::Function<Read> = abi::Function::new("test", "replacement");
    let mut runtime = WasmRuntime::new().unwrap();
    let events = Rc::new(RefCell::new(Vec::<u16>::new()));
    let captured_events = events.clone();
    let (run, alias) = {
        let default = FunctionRef::<_, Read>::new(&mut runtime.context(), move |_, (value,)| {
            captured_events.borrow_mut().push(value);
            u32::from(value) + 10
        });
        let table = runtime.create_table(READ, Some(&default)).unwrap();
        let (run, replacement) = {
            let mut program = Program::new();
            let read = program.import_table(READ);
            RUN.define(&mut program, move |mut body, (slot, value)| {
                let value = body.call_indirect::<I32>(read, slot, &[value.into()])?;
                body.return_(value.add(1))
            })
            .unwrap();
            REPLACEMENT
                .define(&mut program, |body, (value,)| {
                    body.return_(value.unsigned().extend::<I32>().add(100))
                })
                .unwrap();
            let module = runtime
                .compiler()
                .compile(&program.compile().unwrap())
                .await
                .unwrap();
            let instance = runtime
                .instantiate(&module, &[runtime.bind(READ, &table)])
                .unwrap();
            let run = runtime.function(&instance, RUN).unwrap();
            let replacement = runtime.function(&instance, REPLACEMENT).unwrap();
            (run, runtime.import_function(&replacement))
        };
        assert_eq!(runtime.call(&run, (1, 7)), 18);
        let alias = table.clone();
        alias.set(&mut runtime, 1, &replacement).unwrap();
        table.ensure_size(&mut runtime, 4, Some(&default)).unwrap();
        // Growth never rewrites existing slots, including when no growth is needed.
        alias.ensure_size(&mut runtime, 2, None).unwrap();
        (run, alias)
    };
    for (slot, expected) in [(0, 18), (1, 108), (2, 18), (3, 18)] {
        assert_eq!(runtime.call(&run, (slot, 7)), expected);
    }
    assert_eq!(&*events.borrow(), &[7, 7, 7, 7]);
    assert!(alias.ensure_size(&mut runtime, 5, None).is_err());
    assert_eq!(runtime.call(&run, (3, 9)), 20);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn defaults_growth_and_replacement_retain_live_functions() {
    pollster::block_on(check_table_defaults());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn defaults_growth_and_replacement_retain_live_functions() {
    check_table_defaults().await;
}
