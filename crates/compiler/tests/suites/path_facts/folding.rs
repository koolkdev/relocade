//! Canonicalization applies again when branch facts expose simpler operands.

use super::*;
use wasm86_compiler::I64;

fn identities(v8: bool) {
    let module = Fixture::new().function(
        &[Type::I32, Type::I1, Type::I1],
        &[Type::I32; 7],
        |mut body| {
            let input = body.parameter::<I32>(0)?;
            let enabled = body.parameter::<I1>(1)?;
            let other = body.parameter::<I1>(2)?;
            let count = enabled.select(32, 1);
            let conditional = enabled.select(&input, 0);
            let choices = enabled.select(other.select::<I32>(1, 2), 4);
            let results = [
                input.shl(&count),
                input.rotl(count),
                input.eq(&conditional).unsigned().extend::<I32>(),
                input.signed().lt(&conditional).unsigned().extend::<I32>(),
                enabled.select(input.add(7), 0).sub(7),
                other.select(conditional, &input),
                choices.unsigned().lt(3).unsigned().extend::<I32>(),
            ];
            body.if_(enabled, |arm| arm.return_(results))?;
            body.return_([0_u32; 7])
        },
    );
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I32Shl
                | Operator::I32Rotl
                | Operator::I32Eq
                | Operator::I32LtS
                | Operator::I32LtU
                | Operator::I32Add
                | Operator::Select
        )),
        0
    );
    for input in [0, 7, i32::MIN, i32::MAX, -1] {
        for other in [0, 1] {
            check_result(
                &module,
                &[Value::I32(input), Value::I32(1), Value::I32(other)],
                &[input, input, 1, 0, input, input, 1].map(Value::I32),
                v8,
            );
            check_result(
                &module,
                &[Value::I32(input), Value::I32(0), Value::I32(other)],
                &[Value::I32(0); 7],
                v8,
            );
        }
    }
}

fn exposed_constants(v8: bool) {
    let module = Fixture::new().function(&[Type::I32, Type::I1], &[Type::I32; 7], |mut body| {
        let input = body.parameter::<I32>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let difference = input.sub(enabled.select(&input, 0));
        let byte = difference.add(128).truncate::<I8>();
        let results = [
            difference.add(13).mul(7).shl(2),
            byte.signed().extend::<I32>(),
            byte.unsigned().extend::<I32>(),
            byte.clz().unsigned().extend::<I32>(),
            byte.ctz().unsigned().extend::<I32>(),
            byte.popcnt().unsigned().extend::<I32>(),
            difference.add(-1_i32).unsigned().shr(31),
        ];
        body.if_(enabled, |arm| arm.return_(results))?;
        body.return_([0_u32; 7])
    });
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I32Add
                | Operator::I32Mul
                | Operator::I32Shl
                | Operator::I32ShrU
                | Operator::I32Extend8S
                | Operator::I32Clz
                | Operator::I32Ctz
                | Operator::I32Popcnt
        )),
        0
    );
    for input in [0, 255, -1, i32::MIN] {
        check_result(
            &module,
            &[Value::I32(input), Value::I32(1)],
            &[364, -128, 128, 0, 7, 1, 1].map(Value::I32),
            v8,
        );
        check_result(
            &module,
            &[Value::I32(input), Value::I32(0)],
            &[Value::I32(0); 7],
            v8,
        );
    }

    let wide = Fixture::new().function(&[Type::I64, Type::I1], &[Type::I64; 3], |mut body| {
        let input = body.parameter::<I64>(0)?;
        let enabled = body.parameter::<I1>(1)?;
        let difference = input.sub(enabled.select(&input, 0_u64));
        let results = [
            difference.add(u64::MAX).mul(3).rotr(1),
            enabled.select(input.add(u64::MAX), 0_u64).add(1),
            input.shl(enabled.select(64, 1)),
        ];
        body.if_(enabled, |arm| arm.return_(results))?;
        body.return_([0_u64; 3])
    });
    assert_eq!(
        count(&wide, |op| matches!(
            op,
            Operator::I64Add | Operator::I64Mul | Operator::I64Rotr | Operator::I64Shl
        )),
        0
    );
    for input in [0, 0x1_0000_0000, -1, i64::MIN] {
        check_result(
            &wide,
            &[Value::I64(input), Value::I32(1)],
            &[Value::I64(-2), Value::I64(input), Value::I64(input)],
            v8,
        );
        check_result(
            &wide,
            &[Value::I64(input), Value::I32(0)],
            &[Value::I64(0); 3],
            v8,
        );
    }
}

#[test]
fn operand_replacement_reuses_arithmetic_comparison_and_selection_folds() {
    identities(false);
}

#[test]
fn algebraic_folds_feed_constant_evaluation_at_both_carrier_widths() {
    exposed_constants(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_specialized_operations_preserve_results_and_widths() {
    identities(true);
    exposed_constants(true);
}
