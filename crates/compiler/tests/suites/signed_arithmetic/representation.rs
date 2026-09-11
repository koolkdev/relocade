use super::operators;
use crate::fixture::Fixture;
use crate::wasm::{Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{Type, I1, I16, I32, I64, I8};
use wasmparser::Operator;
use Value::{I32 as V32, I64 as V64};

fn count(operations: &[Operator<'_>], predicate: impl Fn(&Operator<'_>) -> bool) -> usize {
    operations
        .iter()
        .filter(|operation| predicate(operation))
        .count()
}

const CHAINS: &[(i32, [Value; 4])] = &[
    (-1, [V64(-1), V64(-1), V64(-1), V32(-1)]),
    (0x1234_5680, [V64(0), V64(-128), V64(22144), V32(-128)]),
    (0x7fff_8001, [V64(-1), V64(1), V64(-32767), V32(1)]),
    (0x1234_01ff, [V64(-1), V64(-1), V64(511), V32(-1)]),
];

fn extension_chains() -> TestModule {
    Fixture::new().function(
        &[Type::I32],
        &[Type::I64, Type::I64, Type::I64, Type::I32],
        |body| {
            let input = body.parameter::<I32>(0)?;
            let bit = input
                .truncate::<I1>()
                .signed()
                .extend::<I8>()
                .signed()
                .extend::<I16>()
                .signed()
                .extend::<I32>()
                .signed()
                .extend::<I64>();
            let byte = input
                .truncate::<I8>()
                .signed()
                .extend::<I16>()
                .signed()
                .extend::<I32>()
                .signed()
                .extend::<I64>();
            let word = input.truncate::<I16>().signed().extend::<I32>();
            body.return_((
                bit,
                byte,
                word.signed().extend::<I64>(),
                word.truncate::<I8>().signed().extend::<I32>(),
            ))
        },
    )
}

#[test]
fn widening_chains_share_sign_interpretations_but_reinterpret_later_truncation() {
    let module = extension_chains();
    let operations = operators(module.bytes());
    assert_eq!(
        count(&operations, |op| matches!(op, Operator::I32Extend8S)),
        2
    );
    assert_eq!(
        count(&operations, |op| matches!(op, Operator::I32Extend16S)),
        1
    );
    assert_eq!(
        count(&operations, |op| matches!(op, Operator::I64ExtendI32S)),
        3
    );
    let mut instance = module.instantiate();
    for (input, expected) in CHAINS {
        assert_eq!(
            instance
                .call_values("run", &[V32(*input)])
                .unwrap()
                .as_slice(),
            expected
        );
    }
}

// Equality with the byte sign extension, inequality, signed/unsigned product, zero.
const PRODUCTS: &[([i32; 2], [i32; 5])] = &[
    ([-128, -128], [0, 1, 16384, 16384, 0]),
    ([-128, 127], [0, 1, -16256, 49280, 0]),
    ([-128, 1], [1, 0, -128, 65408, 0]),
    ([-1, 1], [1, 0, -1, 65535, 0]),
    ([127, 2], [0, 1, 254, 254, 0]),
    ([-128, -1], [0, 1, 128, 128, 0]),
    ([0, -128], [1, 0, 0, 0, 1]),
];

fn signed_byte_product() -> TestModule {
    Fixture::new().function(
        &[Type::I32; 2],
        &[Type::I1, Type::I1, Type::I32, Type::I32, Type::I1],
        |body| {
            let left = body
                .parameter::<I32>(0)?
                .truncate::<I8>()
                .signed()
                .extend::<I16>();
            let right = body
                .parameter::<I32>(1)?
                .truncate::<I8>()
                .signed()
                .extend::<I16>();
            let product = left.mul(right);
            let byte = product.truncate::<I8>().signed().extend::<I16>();
            body.return_((
                product.eq(&byte),
                product.ne(&byte),
                product.signed().extend::<I32>(),
                product.unsigned().extend::<I32>(),
                product.eq(0),
            ))
        },
    )
}

#[test]
fn signed_byte_products_compare_their_carriers_and_keep_unsigned_observers_correct() {
    let module = signed_byte_product();
    let operations = operators(module.bytes());
    assert!(operations.iter().any(|op| matches!(op, Operator::I32Eq)));
    assert!(operations.iter().any(|op| matches!(op, Operator::I32Ne)));
    assert!(!operations
        .iter()
        .any(|op| matches!(op, Operator::I32Xor | Operator::I32Extend16S)));
    let mut instance = module.instantiate();
    for (arguments, expected) in PRODUCTS {
        assert_eq!(
            instance.call_values("run", &arguments.map(V32)).unwrap(),
            expected.map(V32)
        );
    }
}

const DIRTY: &[(i32, [i32; 6])] = &[
    (-1, [1, 0, 0, 1, 254, 1]),
    (128, [1, 0, 1, 0, 0, 0]),
    (0x1234_5680, [1, 0, 1, 0, 0, 0]),
    (0x1234_01ff, [1, 0, 0, 1, 254, 1]),
    (256, [1, 0, 1, 0, 0, 0]),
];

fn dirty_low_bits() -> TestModule {
    Fixture::new().function(
        &[Type::I32],
        &[Type::I1, Type::I1, Type::I1, Type::I1, Type::I32, Type::I1],
        |body| {
            let byte = body.parameter::<I32>(0)?.truncate::<I8>();
            let signed_byte = byte.signed().extend::<I16>().truncate::<I8>();
            let product = byte.mul(2);
            body.return_((
                byte.eq(&signed_byte),
                byte.ne(&signed_byte),
                product.eq(0),
                product.ne(0),
                product.unsigned().extend::<I32>(),
                byte.eq(255),
            ))
        },
    )
}

#[test]
fn equality_and_zero_tests_ignore_dirty_bits_above_the_logical_width() {
    let module = dirty_low_bits();
    let mut instance = module.instantiate();
    for (input, expected) in DIRTY {
        assert_eq!(
            instance.call_values("run", &[V32(*input)]).unwrap(),
            expected.map(V32)
        );
    }
}

const JOINS: &[([i32; 2], [Value; 5])] = &[
    (
        [1, -1],
        [V32(0), V32(1), V32(255), V32(255), V64(4294967295)],
    ),
    (
        [0, -1],
        [V32(1), V32(1), V32(65535), V32(255), V64(4294967295)],
    ),
    ([1, 127], [V32(1), V32(1), V32(127), V32(127), V64(127)]),
    ([0, 127], [V32(1), V32(1), V32(127), V32(127), V64(127)]),
    (
        [1, -2147483648],
        [V32(1), V32(1), V32(0), V32(0), V64(2147483648)],
    ),
    (
        [0, -2147483648],
        [V32(1), V32(1), V32(0), V32(0), V64(2147483648)],
    ),
];

fn mixed_join_representations() -> TestModule {
    Fixture::new().function(
        &[Type::I1, Type::I32],
        &[Type::I1, Type::I1, Type::I32, Type::I32, Type::I64],
        |mut body| {
            let condition = body.parameter::<I1>(0)?;
            let input = body.parameter::<I32>(1)?;
            let byte = input.truncate::<I8>();
            let unsigned = byte.unsigned().extend::<I16>();
            let signed = byte.signed().extend::<I16>();
            let word_join = body.if_value::<I16>(
                &condition,
                |arm| arm.yield_(&unsigned),
                |arm| arm.yield_(&signed),
            )?;
            let byte_join = body.if_value::<I8>(
                condition,
                |arm| arm.yield_(unsigned.truncate::<I8>()),
                |arm| arm.yield_(signed.truncate::<I8>()),
            )?;
            body.return_((
                word_join.eq(&signed),
                byte_join.eq(signed.truncate::<I8>()),
                word_join.unsigned().extend::<I32>(),
                byte_join.unsigned().extend::<I32>(),
                input.unsigned().extend::<I64>(),
            ))
        },
    )
}

#[test]
fn joins_and_unsigned_carrier_widening_preserve_distinct_representations() {
    let module = mixed_join_representations();
    assert!(operators(module.bytes())
        .iter()
        .any(|op| matches!(op, Operator::I64ExtendI32U)));
    let mut instance = module.instantiate();
    for (arguments, expected) in JOINS {
        assert_eq!(
            instance
                .call_values("run", &arguments.map(V32))
                .unwrap()
                .as_slice(),
            expected
        );
    }
}

const SHARED: &[(i32, [Value; 2], [u8; 4])] = &[
    (
        0x1234_5680,
        [V64(-128), V32(65408)],
        [0x80, 0xff, 0xa5, 0xa5],
    ),
    (-1, [V64(-1), V32(65535)], [0xff, 0xff, 0xa5, 0xa5]),
    (127, [V64(127), V32(127)], [0x7f, 0, 0xa5, 0xa5]),
];

fn stored_sign_extension() -> TestModule {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0xa5; 4]);
    fixture.function(&[Type::I32], &[Type::I64, Type::I32], |mut body| {
        let word = body
            .parameter::<I32>(0)?
            .truncate::<I8>()
            .signed()
            .extend::<I16>();
        body.store(memory, 0, &word)?;
        body.return_((
            word.signed().extend::<I64>(),
            word.unsigned().extend::<I32>(),
        ))
    })
}

#[test]
fn a_stored_sign_extension_is_reused_before_widening_the_carrier() {
    let module = stored_sign_extension();
    let operations = operators(module.bytes());
    assert_eq!(
        count(&operations, |op| matches!(op, Operator::I32Extend8S)),
        1
    );
    assert_eq!(
        count(&operations, |op| matches!(op, Operator::I64ExtendI32S)),
        1
    );
    assert!(!operations.iter().any(|op| matches!(
        op,
        Operator::I32Extend16S | Operator::I64Extend8S | Operator::I64Extend16S
    )));
    for (input, expected, memory) in SHARED {
        let mut instance = module.instantiate();
        assert_eq!(
            instance
                .call_values("run", &[V32(*input)])
                .unwrap()
                .as_slice(),
            expected
        );
        assert_eq!(&instance.memory("state")[..4], memory);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn signed_representations_and_logical_observers_execute_in_v8() {
    let module = extension_chains();
    for (input, expected) in CHAINS {
        assert_eq!(
            module.run_v8(&Input::call("run", &[V32(*input)])),
            Observation::returned(expected)
        );
    }
    let module = signed_byte_product();
    for (arguments, expected) in PRODUCTS {
        assert_eq!(
            module.run_v8(&Input::call("run", &arguments.map(V32))),
            Observation::returned(&expected.map(V32))
        );
    }
    let module = dirty_low_bits();
    for (input, expected) in DIRTY {
        assert_eq!(
            module.run_v8(&Input::call("run", &[V32(*input)])),
            Observation::returned(&expected.map(V32))
        );
    }
    let module = mixed_join_representations();
    for (arguments, expected) in JOINS {
        assert_eq!(
            module.run_v8(&Input::call("run", &arguments.map(V32))),
            Observation::returned(expected)
        );
    }
    let module = stored_sign_extension();
    for (input, expected, memory) in SHARED {
        assert_eq!(
            module.run_v8(
                &Input::call("run", &[V32(*input)])
                    .with_memories(&[MemoryBytes::new("state", &[0xa5; 4])])
            ),
            Observation::returned(expected).with_memories(&[MemoryBytes::new("state", memory)])
        );
    }
}
