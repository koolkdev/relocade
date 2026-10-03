use super::*;
use wasm86_compiler::{IntType, I64};

#[test]
fn known_disjuncts_simplify_a_select_condition() {
    let module = Fixture::new().function(&[Type::I1, Type::I1], &[Type::I32], |mut body| {
        let first = body.parameter::<I1>(0)?;
        let second = body.parameter::<I1>(1)?;
        let result = first.or(second).select::<I32>(11, 13);
        body.if_(first, |arm| arm.return_(17))?;
        body.return_(result)
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Or)), 0);
    for (first, second, expected) in [(0, 0, 13), (0, 1, 11), (1, 0, 17), (1, 1, 17)] {
        assert_eq!(
            module.instantiate().call::<i32>((first, second)),
            Ok(expected)
        );
    }
}

#[test]
fn branch_specialization_uses_the_regular_zero_and_one_identities() {
    let module = Fixture::new().function(&[Type::I32, Type::I32], &[Type::I32], |mut body| {
        let stop = body.parameter::<I32>(0)?.ne(0);
        let input = body.parameter::<I32>(1)?;
        let zero = stop.select(7, 0);
        let one = stop.select(0, 1);
        let result = input.or(&zero).xor(&zero).add(&zero).sub(&zero).mul(one);
        body.if_(stop, |arm| arm.return_(17))?;
        body.return_(result)
    });
    assert_eq!(
        count(&module, |op| matches!(
            op,
            Operator::I32Or
                | Operator::I32Xor
                | Operator::I32Add
                | Operator::I32Sub
                | Operator::I32Mul
        )),
        0
    );
    for input in [0, 7, -1, i32::MIN, i32::MAX] {
        assert_eq!(module.instantiate().call::<i32>((0, input)), Ok(input));
        assert_eq!(module.instantiate().call::<i32>((1, input)), Ok(17));
    }
}

#[test]
fn branch_specialization_removes_repeated_bitwise_updates() {
    let module = Fixture::new().function(
        &[Type::I32, Type::I32, Type::I32],
        &[Type::I32, Type::I32],
        |mut body| {
            let choice = body.parameter::<I32>(0)?.ne(0);
            let a = body.parameter::<I32>(1)?;
            let b = body.parameter::<I32>(2)?;
            let operand = choice.select(&a, &b);
            let mut union = a.or(&b);
            let mut intersection = a.and(&b);
            for _ in 0..32 {
                union = union.or(&operand);
                intersection = intersection.and(&operand);
            }
            body.if_(choice, |arm| arm.return_((&union, &intersection)))?;
            body.return_((union, intersection))
        },
    );
    // Each path needs at most one operation per result after choosing its operand.
    assert!(count(&module, |op| matches!(op, Operator::I32Or)) <= 2);
    assert!(count(&module, |op| matches!(op, Operator::I32And)) <= 2);
    let mut instance = module.instantiate();
    for choice in [0, 1] {
        for (a, b, expected) in [
            (0, 0, (0, 0)),
            (0x55, 0x33, (0x77, 0x11)),
            (-1, i32::MIN, (-1, i32::MIN)),
            (i32::MIN, i32::MAX, (-1, 0)),
        ] {
            assert_eq!(instance.call::<(i32, i32)>((choice, a, b)), Ok(expected));
        }
    }
}

#[test]
fn bitwise_identities_preserve_narrow_signed_and_unsigned_observations() {
    let module = Fixture::new().function(
        &[Type::I8, Type::I32, Type::I32],
        &[Type::I32, Type::I64, Type::I32],
        |mut body| {
            let byte = body.parameter::<I8>(0)?;
            let other = body.parameter::<I32>(1)?;
            let choice = body.parameter::<I32>(2)?.ne(0);
            let signed = byte.signed().extend::<I32>();
            let combined = signed.or(&other).truncate::<I8>();
            let operand = choice.select(signed.truncate::<I8>(), other.truncate::<I8>());
            let repeated = combined.or(operand);
            let normalized = combined.unsigned().extend::<I32>();
            body.if_(choice, |arm| {
                arm.return_((
                    repeated.unsigned().extend::<I32>(),
                    repeated.signed().extend::<I64>(),
                    normalized.or(&signed),
                ))
            })?;
            body.return_((
                repeated.unsigned().extend::<I32>(),
                repeated.signed().extend::<I64>(),
                normalized.or(signed),
            ))
        },
    );
    let mut instance = module.instantiate();
    for (byte, other, expected) in [
        (0, 0x100, (0, 0_i64, 0)),
        (0x80, 0x101, (0x81, -127, -127)),
        (0x7f, 0x80, (0xff, -1, 0xff)),
        (0xff, 0, (0xff, -1, -1)),
    ] {
        for choice in [0, 1] {
            assert_eq!(
                instance.call::<(i32, i64, i32)>((byte, other, choice)),
                Ok(expected)
            );
        }
    }
}

fn absorbed_updates<T: IntType>() -> TestModule {
    Fixture::new().function(&[T::TYPE, T::TYPE], &[T::TYPE; 4], |mut body| {
        let previous = body.parameter::<T>(0)?;
        let input = body.parameter::<T>(1)?;
        let computed = input.mul(7);
        let set = computed.and(5);
        let clear = computed.or(!10);
        body.if_(previous.and(15).ne(5), |arm| arm.return_((0, 0, 0, 0)))?;
        body.return_((
            previous.or(&set),
            set.or(&previous),
            previous.and(&clear),
            clear.and(previous),
        ))
    })
}

fn absorbed_results(v8: bool) {
    for (ty, module) in [
        (Type::I32, absorbed_updates::<I32>()),
        (Type::I64, absorbed_updates::<I64>()),
    ] {
        assert_eq!(
            count(&module, |op| matches!(
                op,
                Operator::I32Mul | Operator::I64Mul | Operator::I32Or | Operator::I64Or
            )),
            0
        );
        // Only the guard's mask survives; neither AND result needs calculation.
        assert_eq!(
            count(&module, |op| matches!(
                op,
                Operator::I32And | Operator::I64And
            )),
            1
        );
        let value = |bits: i64| match ty {
            Type::I32 => Value::I32(bits as i32),
            _ => Value::I64(bits),
        };
        for previous in [0, 5, 15, 0x105, -11, i64::MIN + 5] {
            for input in [0, 1, -1, i64::MAX] {
                let expected = if previous & 15 == 5 { previous } else { 0 };
                check_result(
                    &module,
                    &[value(previous), value(input)],
                    &[value(expected); 4],
                    v8,
                );
            }
        }
    }
}

#[test]
fn known_bits_absorb_updates_in_either_operand_order() {
    absorbed_results(false);
}

#[test]
#[ignore = "requires Node.js with V8"]
fn v8_known_bits_absorb_updates_in_either_operand_order() {
    absorbed_results(true);
}

#[test]
fn absorbing_a_sticky_update_preserves_the_byte_and_the_update_call() {
    use crate::{fixture::signature, wasm::Call};

    let mut fixture = Fixture::new();
    let raised = fixture.callback("raised", signature(&[], &[Type::I1]), &[Value::I32(1)]);
    let module = fixture.function(&[Type::I8], &[Type::I8], |mut body| {
        let previous = body.parameter::<I8>(0)?;
        let update = body.call::<I1>(raised, &[])?;
        body.if_(previous.truncate::<I1>().eq(false), |arm| arm.return_(0))?;
        body.return_(previous.or(update.unsigned().extend::<I8>()))
    });
    assert_eq!(count(&module, |op| matches!(op, Operator::I32Or)), 0);
    for previous in [0, 1, 0x81, 0xfe, 0xff] {
        let mut instance = module.instantiate();
        let expected = if previous & 1 != 0 { previous } else { 0 };
        assert_eq!(instance.call::<i32>(previous), Ok(expected));
        assert_eq!(instance.callbacks(), &[Call::new("raised", &[])]);
    }
}
