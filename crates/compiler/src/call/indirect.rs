use crate::{abi, BuildError, Program, Table, Type, I32, I64, I8};

#[test]
fn indirect_calls_preserve_memory_order_and_unused_helper_effects() {
    const MEMORY: abi::Memory = abi::Memory::new("test", "memory", 1);
    const READ: abi::Table<fn(u8) -> u32> = abi::Table::new("test", "read", 1);
    const WRITE: abi::Table<fn(u32)> = abi::Table::new("test", "write", 1);
    let mut program = Program::new();
    let memory = program.import_memory(MEMORY);
    let read = program.import_table(READ);
    let write = program.import_table(WRITE);
    let helper = program.function::<fn(u8) -> u32>(move |mut body, (value,)| {
        let result = body.call_indirect::<I32>(read, 0, &[value.add(1).into()])?;
        body.return_(result)
    });
    let entry = program.function::<fn(u8) -> u64>(move |mut body, (value,)| {
        body.store::<I32>(memory, 0, 10)?;
        let before = body.load::<I32>(memory, 0)?;
        // Unknown effects must propagate through the defined helper.
        body.call::<I32>(helper, &[value.into()])?;
        let after = body.load::<I32>(memory, 0)?;
        body.call_indirect::<()>(write, 0, &[(&after).into()])?;
        body.return_(
            before
                .unsigned()
                .extend::<I64>()
                .shl(32)
                .or(after.unsigned().extend::<I64>()),
        )
    });
    program.export("run", entry).unwrap();

    let engine = wasmtime::Engine::default();
    let mut store = wasmtime::Store::new(&engine, Vec::<(u32, u32)>::new());
    let memory = wasmtime::Memory::new(&mut store, wasmtime::MemoryType::new(1, None)).unwrap();
    let read = wasmtime::Func::wrap(
        &mut store,
        move |mut caller: wasmtime::Caller<'_, Vec<(u32, u32)>>, argument: u32| {
            let value = u32::from_le_bytes(memory.data(&caller)[..4].try_into().unwrap());
            caller.data_mut().push((argument, value));
            memory.write(&mut caller, 0, &15_u32.to_le_bytes()).unwrap();
            99_u32
        },
    );
    let write = wasmtime::Func::wrap(
        &mut store,
        move |mut caller: wasmtime::Caller<'_, Vec<(u32, u32)>>, argument: u32| {
            let value = u32::from_le_bytes(memory.data(&caller)[..4].try_into().unwrap());
            caller.data_mut().push((argument, value));
            memory.write(&mut caller, 0, &17_u32.to_le_bytes()).unwrap();
        },
    );
    let mut linker = wasmtime::Linker::new(&engine);
    linker.define(&store, "test", "memory", memory).unwrap();
    for (name, function) in [("read", read), ("write", write)] {
        let table = wasmtime::Table::new(
            &mut store,
            wasmtime::TableType::new(wasmtime::RefType::FUNCREF, 1, None),
            wasmtime::Ref::Func(Some(function)),
        )
        .unwrap();
        linker.define(&store, "test", name, table).unwrap();
    }
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let run = instance
        .get_typed_func::<u32, u64>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, 255).unwrap(), (10_u64 << 32) | 15);
    assert_eq!(store.data(), &[(0, 10), (15, 15)]);
    assert_eq!(&memory.data(&store)[..4], &17_u32.to_le_bytes());
}

#[test]
fn indirect_calls_share_multiple_results_and_retain_live_table_indices() {
    const UNUSED: abi::Table<fn()> = abi::Table::new("test", "unused", 1);
    const READ: abi::Table<fn(u32) -> (u8, u64)> = abi::Table::new("test", "read", 2);
    let mut program = Program::new();
    program.import_table(UNUSED);
    let read = program.import_table(READ);
    let entry = program.function::<fn(u32) -> u64>(move |mut body, (slot,)| {
        let (byte, wide) = body.call_indirect::<(I8, I64)>(read, slot, &[11.into()])?;
        body.return_(wide.add(byte.unsigned().extend::<I64>()))
    });
    program.export("run", entry).unwrap();
    let engine = wasmtime::Engine::default();
    let mut store = wasmtime::Store::new(&engine, Vec::<u32>::new());
    let first = wasmtime::Func::wrap(
        &mut store,
        |mut caller: wasmtime::Caller<'_, Vec<u32>>, value: u32| {
            caller.data_mut().push(value);
            (7_u32, 0x1_0000_0000_u64)
        },
    );
    let table = wasmtime::Table::new(
        &mut store,
        wasmtime::TableType::new(wasmtime::RefType::FUNCREF, 2, None),
        wasmtime::Ref::Func(Some(first)),
    )
    .unwrap();
    let mut linker = wasmtime::Linker::new(&engine);
    linker.define(&store, "test", "read", table).unwrap();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let instance = linker.instantiate(&mut store, &module).unwrap();
    let run = instance
        .get_typed_func::<u32, u64>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, 1).unwrap(), 0x1_0000_0007);
    let replacement = wasmtime::Func::wrap(
        &mut store,
        |mut caller: wasmtime::Caller<'_, Vec<u32>>, value: u32| {
            caller.data_mut().push(value + 1);
            (8_u32, 0x2_0000_0000_u64)
        },
    );
    table
        .set(&mut store, 1, wasmtime::Ref::Func(Some(replacement)))
        .unwrap();
    assert_eq!(run.call(&mut store, 1).unwrap(), 0x2_0000_0008);
    assert_eq!(run.call(&mut store, 0).unwrap(), 0x1_0000_0007);
    assert_eq!(store.data(), &[11, 12, 11]);
}

#[test]
fn invalid_and_dead_indirect_calls_do_not_retain_their_tables() {
    let mut program = Program::new();
    let read = program.import_table(abi::Table::<fn(u8) -> u8>::new("test", "read", 1));
    let entry = program.function::<fn() -> u32>(move |mut body, ()| {
        assert_eq!(
            body.call_indirect::<I8>(Table(99), 0, &[1.into()]).err(),
            Some(BuildError::UnknownTable)
        );
        assert_eq!(
            body.call_indirect::<I8>(read, 0, &[]).err(),
            Some(BuildError::ArgumentCount {
                expected: 1,
                actual: 0
            })
        );
        assert_eq!(
            body.call_indirect::<()>(read, 0, &[1.into()]).err(),
            Some(BuildError::ResultCount {
                expected: 0,
                actual: 1
            })
        );
        assert_eq!(
            body.call_indirect::<I32>(read, 0, &[1.into()]).err(),
            Some(BuildError::TypeMismatch {
                expected: Type::I32,
                actual: Type::I8
            })
        );
        let word = body.value::<I32>(1)?;
        assert_eq!(
            body.call_indirect::<I8>(read, 0, &[word.into()]).err(),
            Some(BuildError::TypeMismatch {
                expected: Type::I8,
                actual: Type::I32
            })
        );
        let foreign =
            crate::function::tests::capture_from_body(|other| other.value::<I32>(0).unwrap());
        assert_eq!(
            body.call_indirect::<I8>(read, foreign, &[1.into()]).err(),
            Some(BuildError::ForeignBody)
        );
        body.if_(false, |mut branch| {
            branch.call_indirect::<I8>(read, 0, &[1.into()])?;
            Ok(())
        })?;
        body.return_(7)
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
