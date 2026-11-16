use crate::{BuildError, MemoryImport, Program, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

#[test]
fn known_bits_fold_uses_without_reconstructing_the_original_value() {
    let mut program = Program::new();
    let run = program.function::<fn(u32) -> (u32, u32)>(|body, (input,)| {
        let selector = body.assume_bits(&input, 3, 3)?;
        assert!(body
            .assume_bits(&selector, 3, 3)?
            .same_expression(&selector));
        assert!(body
            .assume_bits(&selector, 1, 1)?
            .same_expression(&selector));
        let calculated = selector.or(4);
        assert!(body
            .assume_bits(&calculated, 3, 3)?
            .same_expression(&calculated));
        assert_eq!(
            body.constant_bits(body.assume_bits::<I32>(0, 1, 0)?)?,
            Some(0)
        );
        assert_eq!(body.constant_bits(&input)?, None);
        assert_eq!(body.constant_bits(&selector)?, None);
        assert_eq!(body.constant_bits(selector.and(3))?, Some(3));
        assert_eq!(body.constant_bits(selector.add(1).and(3))?, Some(0));
        assert_eq!(body.constant_bits(selector.and(3).eq(3))?, Some(1));
        body.return_((selector.clone(), selector.and(3)))
    });
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                assert!(!matches!(op.unwrap(), Operator::I32And | Operator::I32Or));
            }
        }
    }
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, bytes).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<u32, (u32, u32)>(&mut store, "run")
        .unwrap();
    for value in [3, 0x1237, u32::MAX] {
        assert_eq!(run.call(&mut store, value).unwrap(), (value, 3));
    }
}

#[test]
fn assumptions_compose_and_respect_logical_widths() {
    let mut program = Program::new();
    let run = program.function::<fn(u64) -> u64>(|body, (input,)| {
        let low = body.assume_bits(&input, 3, 2)?;
        let upper = body.assume_bits(&low, 0xfc, 0x80)?;
        assert_eq!(body.constant_bits(low.and(0xff_u64))?, None);
        assert_eq!(body.constant_bits(upper.and(0xff_u64))?, Some(0x82));
        assert_eq!(body.constant_bits(upper.and(1_u64 << 63))?, None);
        let byte = body.assume_bits(input.truncate::<I8>(), u64::MAX, 0x82)?;
        assert_eq!(body.constant_bits(&byte)?, Some(0x82));
        let high = body.assume_bits(&input, 1_u64 << 63, 1_u64 << 63)?;
        assert_eq!(body.constant_bits(high.unsigned().shr(63))?, Some(1));
        body.return_(byte.unsigned().extend::<I64>())
    });
    program.export("run", run).unwrap();
    Validator::new()
        .validate_all(&program.compile().unwrap())
        .unwrap();
}

#[test]
fn branch_assumptions_stay_local_and_join_results_remain_live() {
    let mut program = Program::new();
    let run = program.function::<fn(u32) -> (u32, u32)>(|mut body, (input,)| {
        let mut escaped = None;
        let joined = body.if_value::<I32>(
            input.and(3).eq(3),
            |arm| {
                let selector = arm.assume_bits(&input, 3, 3)?;
                assert_eq!(arm.constant_bits(selector.and(3))?, Some(3));
                escaped = Some(selector.clone());
                arm.yield_(selector.add(4))
            },
            |arm| {
                assert_eq!(arm.constant_bits(input.and(3))?, None);
                arm.yield_(input.add(8))
            },
        )?;
        let escaped = escaped.unwrap();
        assert_eq!(
            body.assume_bits(&escaped, 0, 0).err(),
            Some(BuildError::OutOfScope)
        );
        assert_eq!(
            body.constant_bits(escaped.and(3)),
            Err(BuildError::OutOfScope)
        );
        assert_eq!(body.constant_bits(input.and(3))?, None);
        assert_eq!(body.constant_bits(joined.and(3))?, None);
        body.return_((joined, input.and(3)))
    });
    program.export("run", run).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<u32, (u32, u32)>(&mut store, "run")
        .unwrap();
    for input in 0..8 {
        assert_eq!(
            run.call(&mut store, input).unwrap(),
            (input + if input & 3 == 3 { 4 } else { 8 }, input & 3),
        );
    }
}

#[test]
fn unreachable_continuations_can_contain_conflicting_assumptions() {
    let mut program = Program::new();
    let run = program.function::<fn() -> u32>(|mut body, ()| {
        body.if_(true, |exit| exit.return_(7))?;
        let impossible = body.assume_bits::<I32>(0, 1, 1)?;
        body.return_(impossible)
    });
    program.export("run", run).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    assert_eq!(
        instance
            .get_typed_func::<(), u32>(&mut store, "run")
            .unwrap()
            .call(&mut store, ())
            .unwrap(),
        7
    );
}

#[test]
fn assumed_reads_keep_their_snapshot_across_writes() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let run = program.function::<fn() -> (u32, u32)>(move |mut body, ()| {
        let before = body.load::<I32>(memory, 0)?;
        let assumed = body.assume_bits(before.add(4), 3, 3)?;
        body.store(memory, 0, body.value::<I32>(0x5670)?)?;
        let after = body.load::<I32>(memory, 0)?;
        assert_eq!(body.constant_bits(assumed.and(3))?, Some(3));
        assert_eq!(body.constant_bits(after.and(3))?, None);
        body.return_((assumed, after.and(3)))
    });
    program.export("run", run).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let memory = wasmtime::Memory::new(&mut store, wasmtime::MemoryType::new(1, None)).unwrap();
    memory
        .write(&mut store, 0, &0x1237_u32.to_le_bytes())
        .unwrap();
    let instance = wasmtime::Instance::new(&mut store, &module, &[memory.into()]).unwrap();
    let run = instance
        .get_typed_func::<(), (u32, u32)>(&mut store, "run")
        .unwrap();
    assert_eq!(run.call(&mut store, ()).unwrap(), (0x123b, 0));
}

#[test]
fn loop_assumptions_describe_each_iterations_current_value() {
    let mut program = Program::new();
    let run = program.function::<fn(u32) -> u32>(|mut body, (count,)| {
        let result = body.loop_::<(I32, I32), I32>(
            (count, 3),
            |mut iteration, labels, (remaining, value)| {
                let value = iteration.assume_bits(value, 3, 3)?;
                assert_eq!(iteration.constant_bits(value.and(3))?, Some(3));
                iteration.if_(remaining.eq(0), |done| done.branch(&labels.exit, &value))?;
                iteration.branch(&labels.again, (remaining.sub(1), value.add(4)))
            },
        )?;
        body.return_(result)
    });
    program.export("run", run).unwrap();
    let engine = wasmtime::Engine::default();
    let module = wasmtime::Module::new(&engine, program.compile().unwrap()).unwrap();
    let mut store = wasmtime::Store::new(&engine, ());
    let instance = wasmtime::Instance::new(&mut store, &module, &[]).unwrap();
    let run = instance
        .get_typed_func::<u32, u32>(&mut store, "run")
        .unwrap();
    for count in [0, 1, 32] {
        assert_eq!(run.call(&mut store, count).unwrap(), 3 + 4 * count);
    }
}

#[test]
fn partial_queries_can_learn_more_bits_later() {
    let mut program = Program::new();
    let run = program.function::<fn(u32, u32) -> u32>(|body, (left, right)| {
        let low = body.assume_bits(left, 0x7f, 1)?;
        let high = right.shl(7);
        let combined = low.or(high);
        let same = body.assume_bits(&combined, 1, 1)?;
        assert!(same.same_expression(&combined));
        assert_eq!(body.constant_bits(combined.and(1))?, Some(1));
        assert_eq!(body.constant_bits(combined.and(0x7f))?, Some(1));
        assert_eq!(body.constant_bits(combined.and(0xff))?, None);
        assert_eq!(body.constant_bits(&combined)?, None);
        body.return_(combined)
    });
    program.export("run", run).unwrap();
    Validator::new()
        .validate_all(&program.compile().unwrap())
        .unwrap();
}
