use crate::{BuildError, Program, Signature, TableImport, Type, I32, I8};

fn table(program: &mut Program, parameter: Type) -> crate::Table {
    program.import_table(TableImport {
        module: "test".into(),
        name: "functions".into(),
        minimum: 2,
        maximum: None,
        signature: Signature {
            parameters: vec![parameter],
            results: vec![Type::I32],
        },
    })
}

#[test]
fn indirect_tail_calls_use_the_live_slot_and_preserve_helper_effects() {
    let mut program = Program::new();
    let table = table(&mut program, Type::I32);
    let signature = Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    };
    let indirect = program.raw_function(signature.clone(), move |body| {
        let slot = body.parameter::<I32>(0)?;
        body.tail_call_indirect(table, slot, &[41.into()])
    });
    let entry = program.raw_function(signature, move |mut body| {
        let slot = body.parameter::<I32>(0)?;
        // Discarding the result must retain the indirect callee's unknown effects.
        body.call::<I32>(indirect, &[slot.into()])?;
        body.return_(7)
    });
    program.export("run", entry).unwrap();
    let mut config = wasmtime::Config::new();
    config.wasm_tail_call(true);
    let engine = wasmtime::Engine::new(&config).unwrap();
    let mut store = wasmtime::Store::new(&engine, Vec::<i32>::new());
    let functions = wasmtime::Table::new(
        &mut store,
        wasmtime::TableType::new(wasmtime::RefType::FUNCREF, 2, None),
        wasmtime::Ref::Func(None),
    )
    .unwrap();
    for index in 0..2 {
        let function = wasmtime::Func::wrap(
            &mut store,
            move |mut caller: wasmtime::Caller<'_, Vec<i32>>, value: i32| {
                caller.data_mut().push(value + index);
                value + index
            },
        );
        functions
            .set(
                &mut store,
                index as u64,
                wasmtime::Ref::Func(Some(function)),
            )
            .unwrap();
    }
    let mut linker = wasmtime::Linker::new(&engine);
    linker
        .define(&store, "test", "functions", functions)
        .unwrap();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let run = instance
        .get_typed_func::<i32, i32>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, 1).unwrap(), 7);
    assert_eq!(run.call(&mut store, 0).unwrap(), 7);
    assert_eq!(store.data(), &[42, 41]);
}

#[test]
fn indirect_calls_check_logical_types_and_prune_unused_tables() {
    let mut program = Program::new();
    let table = self::table(&mut program, Type::I8);
    let invalid = program.function::<fn() -> u32>(move |body, ()| {
        let value = body.value::<I32>(1)?;
        body.tail_call_indirect(table, 0, &[value.into()])
    });
    program.export("invalid", invalid).unwrap();
    let error = program.compile().unwrap_err();
    assert_eq!(
        error,
        BuildError::TypeMismatch {
            expected: Type::I8,
            actual: Type::I32
        }
    );
    let mut program = Program::new();
    let table = self::table(&mut program, Type::I8);
    let entry = program.function::<fn() -> u32>(move |mut body, ()| {
        body.if_(false, |branch| {
            let byte = branch.value::<I8>(3)?;
            branch.tail_call_indirect(table, 1, &[byte.into()])
        })?;
        body.return_(5)
    });
    program.export("run", entry).unwrap();
    let bytes = program.compile().unwrap();
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .unwrap();
    assert!(wasmparser::Parser::new(0)
        .parse_all(&bytes)
        .all(|payload| !matches!(payload.unwrap(), wasmparser::Payload::ImportSection(_))));
}

#[test]
fn table_and_signature_indices_remain_independent_after_pruning() {
    let mut program = Program::new();
    table(&mut program, Type::I8); // omitted before the two live declarations
    let word = table(&mut program, Type::I32);
    let wide = program.import_table(TableImport {
        module: "test".into(),
        name: "wide".into(),
        minimum: 1,
        maximum: Some(1),
        signature: Signature {
            parameters: vec![Type::I64],
            results: vec![Type::I64],
        },
    });
    let entry = program
        .function::<fn() -> u32>(move |body, ()| body.tail_call_indirect(word, 1, &[9.into()]));
    let wide_entry = program.function::<fn() -> u64>(move |body, ()| {
        body.tail_call_indirect(wide, 0, &[0x1_0000_0000u64.into()])
    });
    program.export("word", entry).unwrap();
    program.export("wide", wide_entry).unwrap();
    let mut config = wasmtime::Config::new();
    config.wasm_tail_call(true);
    let engine = wasmtime::Engine::new(&config).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let mut linker = wasmtime::Linker::new(&engine);
    let function = wasmtime::Func::wrap(&mut store, |value: i32| value * 3);
    let words = wasmtime::Table::new(
        &mut store,
        wasmtime::TableType::new(wasmtime::RefType::FUNCREF, 2, None),
        wasmtime::Ref::Func(Some(function)),
    )
    .unwrap();
    let function = wasmtime::Func::wrap(&mut store, |value: i64| value + 1);
    let wides = wasmtime::Table::new(
        &mut store,
        wasmtime::TableType::new(wasmtime::RefType::FUNCREF, 1, Some(1)),
        wasmtime::Ref::Func(Some(function)),
    )
    .unwrap();
    linker.define(&store, "test", "functions", words).unwrap();
    linker.define(&store, "test", "wide", wides).unwrap();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    assert_eq!(
        instance
            .get_typed_func::<(), i32>(&mut store, "word")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        27
    );
    assert_eq!(
        instance
            .get_typed_func::<(), i64>(&mut store, "wide")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        0x1_0000_0001
    );
}
