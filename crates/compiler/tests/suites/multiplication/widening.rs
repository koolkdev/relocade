//! Full products returned as one scalar of twice the operands' logical width.

use super::*;
use wasm86_compiler::DoubleWidth;

fn expected_product(width: u32, left: u64, right: u64, signed: bool) -> Value {
    let modulus = 1_i128 << width;
    let interpret = |bits: u64| {
        let value = i128::from(bits) % modulus;
        if signed && value >= modulus / 2 {
            value - modulus
        } else {
            value
        }
    };
    let product = (interpret(left) * interpret(right)).rem_euclid(modulus * modulus);
    if width == 32 {
        Value::I64(product as i64)
    } else {
        Value::I32(product as i32)
    }
}

fn computed_product<T: DoubleWidth>(signed: bool) -> TestModule
where
    I64: AtLeast<T>,
{
    Fixture::new().expression(&[Type::I64; 2], |body| {
        // Arithmetic can leave bits above T; multiplication must interpret T.
        let left = body.parameter::<I64>(0).unwrap().truncate::<T>().add(1);
        let right = body.parameter::<I64>(1).unwrap().truncate::<T>().sub(1);
        if signed {
            left.signed().mul_wide(right)
        } else {
            left.unsigned().mul_wide(right)
        }
    })
}

#[test]
fn full_scalar_constants_fold_before_and_after_admission() {
    fn check<T: DoubleWidth>(width: u32)
    where
        I64: AtLeast<T>,
    {
        let sign = 1_u64 << (width - 1);
        let mask = (1_u64 << width) - 1;
        for signed in [false, true] {
            for (left, right) in [(mask, mask), (sign, mask), (sign, sign), (sign, 2), (1, 0)] {
                for admitted in [false, true] {
                    let module = Fixture::new().expression(&[], |body| {
                        let left = Val::<T>::from(left as u32);
                        let right = Val::<T>::from(right as u32);
                        let (left, right) = if admitted {
                            (body.value(left).unwrap(), body.value(right).unwrap())
                        } else {
                            (left, right)
                        };
                        if signed {
                            left.signed().mul_wide(right)
                        } else {
                            left.unsigned().mul_wide(right)
                        }
                    });
                    let ops = operators(module.bytes());
                    let expected = expected_product(width, left, right, signed);
                    assert!(
                        matches!(ops.as_slice(), [Operator::I32Const { value }, Operator::Return, Operator::End]
                            if expected == Value::I32(*value))
                            || matches!(ops.as_slice(), [Operator::I64Const { value }, Operator::Return, Operator::End]
                                if expected == Value::I64(*value)),
                        "{left:#x} * {right:#x}, width={width}, signed={signed}, admitted={admitted}"
                    );
                }
            }
        }
    }
    check::<I8>(8);
    check::<I16>(16);
    check::<I32>(32);
}

#[test]
fn full_scalar_products_normalize_dirty_operands_and_keep_both_halves() {
    fn check<T: DoubleWidth>(width: u32)
    where
        I64: AtLeast<T>,
    {
        let sign = 1_i64 << (width - 1);
        let mask = (1_i64 << width) - 1;
        for signed in [false, true] {
            let module = computed_product::<T>(signed);
            assert_eq!(
                operators(module.bytes())
                    .iter()
                    .filter(|op| matches!(op, Operator::I32Mul | Operator::I64Mul))
                    .count(),
                1
            );
            let mut instance = module.instantiate();
            for left in [0_i64, -1, sign - 1, mask, 0x1234_5678_9abc_def0] {
                for right in [0_i64, 1, 2, sign + 1, mask, 0x1234_5678_9abc_def0] {
                    assert_eq!(
                        instance
                            .call_values("run", &[Value::I64(left), Value::I64(right)])
                            .unwrap(),
                        vec![expected_product(
                            width,
                            left.wrapping_add(1) as u64,
                            right.wrapping_sub(1) as u64,
                            signed
                        )],
                        "({left:#x} + 1) * ({right:#x} - 1), width={width}, signed={signed}"
                    );
                }
            }
        }
    }
    check::<I8>(8);
    check::<I16>(16);
    check::<I32>(32);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn full_scalar_products_execute_in_v8() {
    fn check<T: DoubleWidth>(width: u32)
    where
        I64: AtLeast<T>,
    {
        for signed in [false, true] {
            let module = computed_product::<T>(signed);
            let sign = 1_i64 << (width - 1);
            for (left, right) in [(-2_i64, 0_i64), (sign - 1, sign + 1), (sign - 1, 3)] {
                assert_eq!(
                    module.run_v8(&Input::call("run", &[Value::I64(left), Value::I64(right)])),
                    Observation::returned(&[expected_product(
                        width,
                        left.wrapping_add(1) as u64,
                        right.wrapping_sub(1) as u64,
                        signed
                    )])
                );
            }
        }
    }
    check::<I8>(8);
    check::<I16>(16);
    check::<I32>(32);
}
