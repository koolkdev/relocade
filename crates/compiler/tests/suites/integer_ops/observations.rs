//! Narrow results retain carries and borrows while discarding redundant masks.
use super::*;
use crate::wasm::{Input, Observation};

fn narrow_arithmetic(v8: bool) {
    let module = Fixture::new().function(&[Type::I32; 2], &[Type::I32; 3], |body| {
        let left = body.parameter::<I32>(0)?.and(255);
        let right = body.parameter::<I32>(1)?;
        body.return_((
            left.add(&right).truncate::<I8>().unsigned().extend::<I32>(),
            left.sub(&right).truncate::<I8>().unsigned().extend::<I32>(),
            left.mul(right).truncate::<I8>().unsigned().extend::<I32>(),
        ))
    });
    // Each result is masked once. The earlier mask on the left input is redundant.
    assert_eq!(inspect(module.bytes()).masks, 3);
    for (left, right) in [
        (0x1234_00ff_i32, 1_i32),
        (0x1234_0080, 255),
        (-1, -1),
        (0, 1),
        (0x1234_007f, 0x5678_0081),
    ] {
        let arguments = [Value::I32(left), Value::I32(right)];
        let expected = [
            left.wrapping_add(right),
            left.wrapping_sub(right),
            left.wrapping_mul(right),
        ]
        .map(|bits| Value::I32(bits & 255));
        if v8 {
            assert_eq!(
                module.run_v8(&Input::call("run", &arguments)),
                Observation::returned(&expected)
            );
        } else {
            assert_eq!(
                module.instantiate().call_values("run", &arguments).unwrap(),
                expected
            );
        }
    }
}

#[test]
fn narrow_arithmetic_preserves_carries_borrows_and_products() {
    narrow_arithmetic(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_narrow_arithmetic_preserves_carries_borrows_and_products() {
    narrow_arithmetic(true);
}
