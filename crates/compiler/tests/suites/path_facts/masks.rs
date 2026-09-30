//! Explicit masks refold when branch facts expose their operands.

use super::*;
use wasm86_compiler::{IntType, Val, I16, I64};

fn check_cancellation<T: IntType>(mask: impl Into<Val<T>>, cases: &[(Value, Value)], v8: bool) {
    let mask = mask.into();
    let module = Fixture::new().function(&[T::TYPE, Type::I1], &[T::TYPE], |mut body| {
        let input = body.parameter::<T>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let decremented = enabled.select(input.add(&mask).and(&mask), 0);
        let restored = decremented.add(1).and(&mask);
        body.if_(enabled, |arm| arm.return_(restored))?;
        body.return_(0)
    });
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I32Add | Operator::I64Add
        )),
        0
    );
    let zero = if T::TYPE == Type::I64 {
        Value::I64(0)
    } else {
        Value::I32(0)
    };
    for &(input, expected) in cases {
        check_result(&module, &[input, Value::I32(1)], &[expected], v8);
        check_result(&module, &[input, Value::I32(0)], &[zero], v8);
    }
}

fn cancellation_at_each_width(v8: bool) {
    check_cancellation::<I1>(
        1,
        &[
            (Value::I32(0), Value::I32(0)),
            (Value::I32(1), Value::I32(1)),
        ],
        v8,
    );
    check_cancellation::<I8>(
        255,
        &[
            (Value::I32(0), Value::I32(0)),
            (Value::I32(128), Value::I32(128)),
            (Value::I32(255), Value::I32(255)),
        ],
        v8,
    );
    check_cancellation::<I16>(
        65535,
        &[
            (Value::I32(0), Value::I32(0)),
            (Value::I32(32768), Value::I32(32768)),
            (Value::I32(65535), Value::I32(65535)),
        ],
        v8,
    );
    check_cancellation::<I32>(
        65535,
        &[
            (Value::I32(65536), Value::I32(0)),
            (Value::I32(65537), Value::I32(1)),
            (Value::I32(-1), Value::I32(65535)),
        ],
        v8,
    );
    check_cancellation::<I64>(
        0x1_ffff_ffff_u64,
        &[
            (Value::I64(0x1_0000_0001), Value::I64(0x1_0000_0001)),
            (Value::I64(i64::MIN), Value::I64(0)),
            (Value::I64(-1), Value::I64(0x1_ffff_ffff)),
        ],
        v8,
    );
}

fn guarded_chain(v8: bool) {
    let module = Fixture::new().function(&[Type::I32, Type::I1], &[Type::I32], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let mut position = input.and(7);
        for _ in 0..32 {
            let decremented = enabled.select(
                position.add(7).and(7).truncate::<I8>(),
                position.truncate::<I8>(),
            );
            position = decremented.unsigned().extend::<I32>().add(1).and(7);
        }
        body.if_(enabled, |arm| arm.return_(position))?;
        body.return_(0)
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Add)), 0);
    for (input, expected) in [(0, 0), (7, 7), (8, 0), (255, 7), (i32::MIN, 0), (-1, 7)] {
        check_result(
            &module,
            &[Value::I32(input), Value::I32(1)],
            &[Value::I32(expected)],
            v8,
        );
        check_result(
            &module,
            &[Value::I32(input), Value::I32(0)],
            &[Value::I32(0)],
            v8,
        );
    }
}

fn observation_boundaries(v8: bool) {
    let module = Fixture::new().function(&[Type::I32, Type::I1], &[Type::I32; 5], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let masked = enabled.select(input.add(5).and(15), 0).add(6).and(7);
        let sparse = enabled.select(input.add(5).and(5), 0).add(6).and(7);
        let signed = enabled
            .select(input.add(120).and(255), 0)
            .truncate::<I8>()
            .signed()
            .extend::<I32>()
            .add(10)
            .and(511);
        let narrowed = enabled
            .select(input.add(65530).and(65535), 0)
            .truncate::<I8>()
            .unsigned()
            .extend::<I32>()
            .add(7)
            .and(511);
        let carry = enabled.select(input.and(7), 0).add(1).and(15);
        body.if_(enabled, |arm| {
            arm.return_([masked, sparse, signed, narrowed, carry])
        })?;
        body.return_([0_u32; 5])
    });
    for (input, expected) in [
        (0, [3, 3, 130, 257, 1]),
        (1, [4, 2, 131, 258, 2]),
        (7, [2, 2, 137, 8, 8]),
        (8, [3, 3, 394, 9, 1]),
        (127, [2, 2, 1, 128, 8]),
        (255, [2, 2, 129, 256, 8]),
        (-1, [2, 2, 129, 256, 8]),
    ] {
        check_result(
            &module,
            &[Value::I32(input), Value::I32(1)],
            &expected.map(Value::I32),
            v8,
        );
        check_result(
            &module,
            &[Value::I32(input), Value::I32(0)],
            &[Value::I32(0); 5],
            v8,
        );
    }

    let wide = Fixture::new().function(&[Type::I64, Type::I1], &[Type::I64], |mut body| {
        let input = body.parameter::<I64>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let masked = enabled
            .select(input.add(5), 0_u64)
            .truncate::<I32>()
            .unsigned()
            .extend::<I64>()
            .add(1)
            .and(0x1_ffff_ffff_u64);
        body.if_(enabled, |arm| arm.return_(masked))?;
        body.return_(0_u64)
    });
    for (input, expected) in [
        (0, 6),
        (0x1_0000_0000, 6),
        (0xffff_fffa, 0x1_0000_0000),
        (-1, 5),
    ] {
        check_result(
            &wide,
            &[Value::I64(input), Value::I32(1)],
            &[Value::I64(expected)],
            v8,
        );
        check_result(
            &wide,
            &[Value::I64(input), Value::I32(0)],
            &[Value::I64(0)],
            v8,
        );
    }
}

#[test]
fn branch_facts_cancel_masked_offsets_at_each_logical_width() {
    cancellation_at_each_width(false);
}

#[test]
fn branch_facts_collapse_guarded_mask_chains_through_byte_views() {
    guarded_chain(false);
}

#[test]
fn refolding_masks_preserves_carries_sparse_masks_and_conversion_boundaries() {
    observation_boundaries(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_refolded_masks_preserve_results_and_observation_boundaries() {
    cancellation_at_each_width(true);
    guarded_chain(true);
    observation_boundaries(true);
}
