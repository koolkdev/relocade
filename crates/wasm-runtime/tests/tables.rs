use relocade_compiler::{abi, Program, I32};
use relocade_wasm_runtime::{Backend, WasmBackend, WasmCompiler, WasmContext};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

async fn check_table_defaults() {
    type Read = fn(u16) -> u32;
    const READ: abi::Table<Read> = {
        let mut table = abi::Table::new("test", "read", 2);
        table.maximum = Some(4);
        table
    };
    const RUN: abi::Function<fn(u32, u16) -> u32> = abi::Function::new("test", "run");
    const REPLACEMENT: abi::Function<Read> = abi::Function::new("test", "replacement");
    let mut backend = Backend::with_state(Vec::<u16>::new()).unwrap();
    let (run, alias) = {
        let default = backend.host_function::<Read>(Box::new(|mut context, value| {
            context.state_mut().push(value);
            u32::from(value) + 10
        }));
        let table = backend.create_table(READ, Some(&default)).unwrap();
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
            let module = backend
                .compiler()
                .compile(&program.compile().unwrap())
                .await
                .unwrap();
            let instance = backend
                .instantiate(&module, &[backend.bind(READ, &table)])
                .await
                .unwrap();
            let run = backend.function(&instance, RUN).unwrap();
            let replacement = backend.function(&instance, REPLACEMENT).unwrap();
            (run, backend.import_function(&replacement))
        };
        assert_eq!(backend.call(&run, (1, 7)), 18);
        let alias = table.clone();
        alias.set(&mut backend, 1, &replacement).unwrap();
        table.ensure_size(&mut backend, 4, Some(&default)).unwrap();
        // Growth never rewrites existing slots, including when no growth is needed.
        alias.ensure_size(&mut backend, 2, None).unwrap();
        (run, alias)
    };
    for (slot, expected) in [(0, 18), (1, 108), (2, 18), (3, 18)] {
        assert_eq!(backend.call(&run, (slot, 7)), expected);
    }
    assert_eq!(&*backend.state(), &[7, 7, 7, 7]);
    assert!(alias.ensure_size(&mut backend, 5, None).is_err());
    assert_eq!(backend.call(&run, (3, 9)), 20);
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
