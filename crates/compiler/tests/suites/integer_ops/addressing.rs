use crate::fixture::Fixture;
use wasm86_compiler::{Type, I1, I16, I32, I64, I8};

#[test]
fn computed_i32_shift_counts_wrap_at_32_bits() {
    let module = Fixture::new().expression(&[Type::I32, Type::I32], |body| {
        body.parameter::<I32>(0)
            .unwrap()
            .shl(body.parameter::<I32>(1).unwrap())
    });
    for (count, expected) in [(31, -2147483648), (32, 1), (33, 2), (-1, -2147483648)] {
        assert_eq!(
            module.instantiate().call::<i32>((1, count)).unwrap(),
            expected
        );
    }
}

#[test]
fn computed_i64_shifts_accept_i32_counts_and_wrap_at_64_bits() {
    let module = Fixture::new().expression(&[Type::I64, Type::I32], |body| {
        body.parameter::<I64>(0)
            .unwrap()
            .shl(body.parameter::<I32>(1).unwrap())
    });
    for (count, expected) in [(63, -9223372036854775808), (64, 1), (65, 2)] {
        assert_eq!(
            module.instantiate().call::<i64>((1_i64, count)).unwrap(),
            expected,
        );
    }
}

#[test]
fn computed_i8_shifts_truncate_results_and_wrap_counts_at_32_bits() {
    let module = Fixture::new().expression(&[Type::I8, Type::I32], |body| {
        body.parameter::<I8>(0)
            .unwrap()
            .shl(body.parameter::<I32>(1).unwrap())
    });
    for (count, expected) in [(1, 0), (32, 128)] {
        assert_eq!(
            module.instantiate().call::<i32>((128, count)).unwrap(),
            expected
        );
    }
}

#[test]
fn signed_i8_extension_to_i32_preserves_the_sign() {
    let module = Fixture::new().expression(&[Type::I8], |body| {
        body.parameter::<I8>(0).unwrap().signed().extend::<I32>()
    });
    for (input, expected) in [(127, 127), (128, -128), (255, -1)] {
        assert_eq!(module.instantiate().call::<i32>(input).unwrap(), expected);
    }
}

#[test]
fn signed_i8_extension_to_i16_normalizes_the_i32_carrier() {
    let module = Fixture::new().expression(&[Type::I8], |body| {
        body.parameter::<I8>(0).unwrap().signed().extend::<I16>()
    });
    assert_eq!(module.instantiate().call::<i32>(128).unwrap(), 65408);
}

#[test]
fn signed_i8_extension_normalizes_after_wrapping_arithmetic() {
    let module = Fixture::new().expression(&[Type::I8], |body| {
        body.parameter::<I8>(0)
            .unwrap()
            .add(1)
            .signed()
            .extend::<I32>()
    });
    for (input, expected) in [(127, -128), (255, 0)] {
        assert_eq!(module.instantiate().call::<i32>(input).unwrap(), expected);
    }
}

#[test]
fn signed_i1_extension_to_i64_preserves_the_sign() {
    let module = Fixture::new().expression(&[Type::I1], |body| {
        body.parameter::<I1>(0).unwrap().signed().extend::<I64>()
    });
    for (input, expected) in [(1, -1), (0, 0)] {
        assert_eq!(module.instantiate().call::<i64>(input).unwrap(), expected);
    }
}

#[test]
fn signed_i16_extension_to_i64_preserves_the_sign() {
    let module = Fixture::new().expression(&[Type::I16], |body| {
        body.parameter::<I16>(0).unwrap().signed().extend::<I64>()
    });
    assert_eq!(module.instantiate().call::<i64>(32768).unwrap(), -32768);
}

#[test]
fn signed_i32_extension_to_i64_preserves_the_sign() {
    let module = Fixture::new().expression(&[Type::I32], |body| {
        body.parameter::<I32>(0).unwrap().signed().extend::<I64>()
    });
    assert_eq!(
        module.instantiate().call::<i64>(-2147483648).unwrap(),
        -2147483648,
    );
}
