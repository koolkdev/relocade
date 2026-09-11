use crate::fixture::{signature, Fixture};
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{Type, I1, I16, I32, I64, I8};
use wasmparser::{Operator, Parser, Payload, Validator};

fn operators(bytes: &[u8]) -> Vec<Operator<'_>> {
    Validator::new().validate_all(bytes).unwrap();
    Parser::new(0)
        .parse_all(bytes)
        .filter_map(|payload| match payload.unwrap() {
            Payload::CodeSectionEntry(body) => Some(
                body.get_operators_reader()
                    .unwrap()
                    .into_iter()
                    .map(Result::unwrap),
            ),
            _ => None,
        })
        .flatten()
        .collect()
}

fn loads<'a>(operators: &'a [Operator<'_>]) -> Vec<(&'static str, &'a wasmparser::MemArg)> {
    operators
        .iter()
        .filter_map(|operator| {
            Some(match operator {
                Operator::I32Load8U { memarg } => ("i32.load8_u", memarg),
                Operator::I32Load8S { memarg } => ("i32.load8_s", memarg),
                Operator::I32Load16U { memarg } => ("i32.load16_u", memarg),
                Operator::I32Load16S { memarg } => ("i32.load16_s", memarg),
                Operator::I32Load { memarg } => ("i32.load", memarg),
                Operator::I64Load8S { memarg } => ("i64.load8_s", memarg),
                Operator::I64Load16S { memarg } => ("i64.load16_s", memarg),
                Operator::I64Load32S { memarg } => ("i64.load32_s", memarg),
                Operator::I64Load { memarg } => ("i64.load", memarg),
                _ => return None,
            })
        })
        .collect()
}

const SIGNED_BYTES: &[u8] = &[
    0xa5, 0x80, 0, 0x80, 0xff, 0x5a, 0xff, 0x7f, 0, 0, 0, 0x80, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0x7f,
    0xff, 0x7f, 0x80, 0x5a, 0, 0x80, 0xff, 0xff, 0xff, 0x7f, 0xa5, 0xa5, 0xa5, 0xa5,
];
const SIGNED_CASES: &[(i32, [Value; 5])] = &[
    (
        0,
        [
            Value::I32(-128),
            Value::I32(-32768),
            Value::I64(-1),
            Value::I64(32767),
            Value::I64(-2147483648),
        ],
    ),
    (
        16,
        [
            Value::I32(127),
            Value::I32(32767),
            Value::I64(-128),
            Value::I64(-32768),
            Value::I64(2147483647),
        ],
    ),
];

fn signed_reads() -> TestModule {
    let mut fixture = Fixture::new();
    let address_memory = fixture.memory("address", &[0; 4]);
    let memory = fixture.memory("state", SIGNED_BYTES);
    fixture.function(
        &[Type::I32],
        &[Type::I32, Type::I32, Type::I64, Type::I64, Type::I64],
        |mut body| {
            let address = body
                .parameter::<I32>(0)?
                .add(body.load::<I32>(address_memory, 0)?);
            let byte = body.load_at::<I8>(memory, &address, 1)?;
            let word = body.load_at::<I16>(memory, &address, 2)?;
            let wide_byte = body.load_at::<I8>(memory, &address, 4)?;
            let wide_word = body.load_at::<I16>(memory, &address, 6)?;
            let wide_dword = body.load_at::<I32>(memory, &address, 8)?;
            body.return_((
                byte.signed().extend::<I32>(),
                word.signed().extend::<I32>(),
                wide_byte.signed().extend::<I64>(),
                wide_word.signed().extend::<I64>(),
                wide_dword.signed().extend::<I64>(),
            ))
        },
    )
}

#[test]
fn sole_signed_read_uses_preserve_the_address_memory_and_access_width() {
    let module = signed_reads();
    let operations = operators(module.bytes());
    let loads = loads(&operations);
    assert_eq!(loads.len(), 6);
    for ((name, argument), (expected, memory, offset, align)) in loads.iter().zip([
        ("i32.load", 0, 0, 2),
        ("i32.load8_s", 1, 1, 0),
        ("i32.load16_s", 1, 2, 1),
        ("i64.load8_s", 1, 4, 0),
        ("i64.load16_s", 1, 6, 1),
        ("i64.load32_s", 1, 8, 2),
    ]) {
        assert_eq!(*name, expected);
        assert_eq!(argument.memory, memory);
        assert_eq!(argument.offset, offset);
        assert_eq!(argument.align, align);
    }
    assert!(!operations.iter().any(|operation| matches!(
        operation,
        Operator::I32Extend8S | Operator::I32Extend16S | Operator::I64ExtendI32S
    )));
    let mut instance = module.instantiate();
    for (address, expected) in SIGNED_CASES {
        assert_eq!(
            instance
                .call_values("run", &[Value::I32(*address)])
                .unwrap()
                .as_slice(),
            expected
        );
    }
}

const SHARED_BYTES: &[u8] = &[0xfe, 0xa5, 0x5a, 0xc3, 0, 0, 0, 0, 0, 0, 0, 0];
const SHARED_AFTER: &[u8] = &[0xfe, 0xa5, 0x5a, 0xc3, 0, 0, 0, 0, 0xfe, 0xff, 0xff, 0xff];

fn repeated_signed_value() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", SHARED_BYTES);
    fixture.function(&[], &[Type::I32], |mut body| {
        let value = body.load::<I8>(memory, 0)?.signed().extend::<I32>();
        body.store(memory, 8, &value)?;
        body.return_(value.add(&value))
    })
}

#[test]
fn repeated_signed_uses_share_the_extended_result_from_one_signed_load() {
    let module = repeated_signed_value();
    let operations = operators(module.bytes());
    assert_eq!(
        loads(&operations)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["i32.load8_s"]
    );
    assert_eq!(
        operations
            .iter()
            .filter(|operation| matches!(
                operation,
                Operator::LocalSet { .. } | Operator::LocalTee { .. }
            ))
            .count(),
        1
    );
    let mut instance = module.instantiate();
    assert_eq!(instance.call::<i32>(()).unwrap(), -4);
    assert_eq!(
        &instance.memory("state")[..SHARED_AFTER.len()],
        SHARED_AFTER
    );
}

const MIXED_BYTES: &[u8] = &[0xfe, 0xa5, 0x5a, 0xc3];
const MIXED_AFTER: &[u8] = &[7, 0xa5, 0x5a, 0xc3];
const MIXED_RESULT: &[Value] = &[Value::I32(254), Value::I32(-2), Value::I32(7)];

fn mixed_snapshot() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", MIXED_BYTES);
    fixture.function(&[], &[Type::I32; 3], |mut body| {
        let before = body.load::<I8>(memory, 0)?;
        body.store::<I8>(memory, 0, 7)?;
        let after = body.load::<I8>(memory, 0)?;
        body.return_((
            before.unsigned().extend::<I32>(),
            before.signed().extend::<I32>(),
            after.signed().extend::<I32>(),
        ))
    })
}

#[test]
fn mixed_signed_and_unsigned_uses_share_the_read_before_an_overlapping_store() {
    let module = mixed_snapshot();
    let operations = operators(module.bytes());
    let accesses: Vec<_> = operations
        .iter()
        .filter_map(|operation| match operation {
            Operator::I32Load8U { .. } => Some("unsigned snapshot"),
            Operator::I32Store8 { .. } => Some("overwrite"),
            Operator::I32Extend8S => Some("interpret snapshot sign"),
            Operator::I32Load8S { .. } => Some("fresh signed read"),
            _ => None,
        })
        .collect();
    assert_eq!(
        accesses,
        [
            "unsigned snapshot",
            "overwrite",
            "interpret snapshot sign",
            "fresh signed read"
        ]
    );
    let mut instance = module.instantiate();
    assert_eq!(instance.call_values("run", &[]).unwrap(), MIXED_RESULT);
    assert_eq!(&instance.memory("state")[..4], MIXED_AFTER);
}

const CALL_BYTES: &[u8] = &[0x80, 0xa5, 0x5a, 0xc3];
const CALL_AFTER: &[u8] = &[0x80, 0, 0x5a, 0xc3];
const CALL_RESULT: &[Value] = &[Value::I64(-23168), Value::I64(128)];

fn snapshot_across_call() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", CALL_BYTES);
    let run = fixture.program.declare(signature(&[], &[Type::I64; 2]));
    let overwrite = fixture.program.declare(signature(&[], &[]));
    let mut body = fixture.program.define(run).unwrap();
    let before = body.load::<I16>(memory, 0).unwrap();
    body.call::<()>(overwrite, &[]).unwrap();
    let after = body.load::<I16>(memory, 0).unwrap();
    body.return_((
        before.signed().extend::<I64>(),
        after.signed().extend::<I64>(),
    ))
    .unwrap();
    let mut body = fixture.program.define(overwrite).unwrap();
    body.store::<I8>(memory, 1, 0).unwrap();
    body.return_(()).unwrap();
    fixture.finish(run)
}

#[test]
fn signed_extension_keeps_the_word_snapshot_across_a_call_that_writes_its_high_byte() {
    let module = snapshot_across_call();
    let operations = operators(module.bytes());
    assert_eq!(
        loads(&operations)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["i32.load16_u", "i64.load16_s"]
    );
    let mut instance = module.instantiate();
    assert_eq!(instance.call_values("run", &[]).unwrap(), CALL_RESULT);
    assert_eq!(&instance.memory("state")[..4], CALL_AFTER);
}

const BRANCH_BYTES: &[u8] = &[0x80, 0xff, 0x5a, 0xc3];
const BRANCH_CASES: &[(i32, [u8; 4])] = &[(0, [11, 0, 0x5a, 0xc3]), (1, [7, 0, 0x5a, 0xc3])];

fn snapshot_across_branches() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", BRANCH_BYTES);
    fixture.function(&[Type::I1], &[Type::I64], |mut body| {
        let condition = body.parameter::<I1>(0)?;
        let before = body.load::<I16>(memory, 0)?;
        let result = body.if_value::<I64>(
            condition,
            |mut arm| {
                arm.store::<I16>(memory, 0, 7)?;
                arm.yield_(before.signed().extend::<I64>())
            },
            |mut arm| {
                arm.store::<I16>(memory, 0, 11)?;
                arm.yield_(before.signed().extend::<I64>())
            },
        )?;
        body.return_(result)
    })
}

#[test]
fn signed_branch_uses_keep_one_snapshot_before_either_arm_overwrites_it() {
    let module = snapshot_across_branches();
    let operations = operators(module.bytes());
    assert_eq!(
        loads(&operations)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        ["i32.load16_u"]
    );
    for (condition, memory) in BRANCH_CASES {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i64>((*condition,)).unwrap(), -128);
        assert_eq!(&instance.memory("state")[..4], memory);
    }
}

const CONVERT_BYTES: &[u8] = &[0x80, 1, 0x55, 0x80, 0, 0, 0, 0];
const CONVERT_RESULT: &[Value] = &[
    Value::I32(-128),
    Value::I32(128),
    Value::I32(-128),
    Value::I64(384),
    Value::I64(-2141912704),
];

fn converted_read_signs() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", CONVERT_BYTES);
    fixture.function(
        &[],
        &[Type::I32, Type::I32, Type::I32, Type::I64, Type::I64],
        |mut body| {
            let narrowed = body.load::<I16>(memory, 0)?.truncate::<I8>();
            let widened = body.load::<I8>(memory, 0)?.unsigned().extend::<I16>();
            let same_width = body
                .load::<I8>(memory, 0)?
                .unsigned()
                .extend::<I16>()
                .truncate::<I8>();
            let narrow_word = body.load::<I32>(memory, 0)?.truncate::<I16>();
            let narrow_dword = body.load::<I64>(memory, 0)?.truncate::<I32>();
            body.return_((
                narrowed.signed().extend::<I32>(),
                widened.signed().extend::<I32>(),
                same_width.signed().extend::<I32>(),
                narrow_word.signed().extend::<I64>(),
                narrow_dword.signed().extend::<I64>(),
            ))
        },
    )
}

#[test]
fn logical_conversions_preserve_the_observed_sign_and_the_original_read_width() {
    let module = converted_read_signs();
    let operations = operators(module.bytes());
    assert_eq!(
        loads(&operations)
            .iter()
            .map(|(name, _)| *name)
            .collect::<Vec<_>>(),
        [
            "i32.load16_u",
            "i32.load8_u",
            "i32.load8_s",
            "i32.load",
            "i64.load"
        ]
    );
    assert_eq!(
        module.instantiate().call_values("run", &[]).unwrap(),
        CONVERT_RESULT
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn signed_load_widths_sharing_and_logical_conversions_execute_in_v8() {
    let module = signed_reads();
    for (address, expected) in SIGNED_CASES {
        let memories = [
            MemoryBytes::new("address", &[0; 4]),
            MemoryBytes::new("state", SIGNED_BYTES),
        ];
        assert_eq!(
            module.run_v8(&Input::call("run", &[Value::I32(*address)]).with_memories(&memories)),
            Observation::returned(expected).with_memories(&memories)
        );
    }
    for (module, initial, expected, memory) in [
        (
            repeated_signed_value(),
            SHARED_BYTES,
            &[Value::I32(-4)][..],
            SHARED_AFTER,
        ),
        (
            converted_read_signs(),
            CONVERT_BYTES,
            CONVERT_RESULT,
            CONVERT_BYTES,
        ),
    ] {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", initial)])
            ),
            Observation::returned(expected).with_memories(&[MemoryBytes::new("state", memory)])
        );
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn signed_read_snapshots_execute_in_v8_across_stores_calls_and_branches() {
    for (module, initial, expected, memory) in [
        (mixed_snapshot(), MIXED_BYTES, MIXED_RESULT, MIXED_AFTER),
        (snapshot_across_call(), CALL_BYTES, CALL_RESULT, CALL_AFTER),
    ] {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", initial)])
            ),
            Observation::returned(expected).with_memories(&[MemoryBytes::new("state", memory)])
        );
    }
    let module = snapshot_across_branches();
    for (condition, memory) in BRANCH_CASES {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[Value::I32(*condition)])
                    .with_memories(&[MemoryBytes::new("state", BRANCH_BYTES)])
            ),
            Observation::returned(&[Value::I64(-128)])
                .with_memories(&[MemoryBytes::new("state", memory)])
        );
    }
}
