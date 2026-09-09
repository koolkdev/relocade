use crate::fixture::Fixture;
use crate::wasm::TestModule;
use wasm86_compiler::{IntType, Program, Signature, Type, Val, I1, I16, I32, I64, I8};

#[test]
fn dword_literals_preserve_their_bits() {
    for (bits, expected) in [
        (0_u32, 0),
        (0x8000_0000, i32::MIN),
        (0x7fff_ffff, i32::MAX),
        (u32::MAX, -1),
    ] {
        let module = Fixture::new().expression(&[], |_| Val::<I32>::from(bits));
        assert_eq!(module.instantiate().call::<i32>(()).unwrap(), expected);
    }
}

#[test]
fn qword_literals_preserve_their_bits() {
    for (bits, expected) in [
        (0_u64, 0),
        (0x8000_0000_0000_0000, i64::MIN),
        (0x7fff_ffff_ffff_ffff, i64::MAX),
        (u64::MAX, -1),
    ] {
        let module = Fixture::new().expression(&[], |_| Val::<I64>::from(bits));
        assert_eq!(module.instantiate().call::<i64>(()).unwrap(), expected);
    }
}

#[test]
fn native_literals_keep_their_signed_or_unsigned_value() {
    let signed32 = Fixture::new().expression(&[], |_| Val::<I32>::from(-2147483647));
    assert_eq!(signed32.instantiate().call::<i32>(()).unwrap(), -2147483647);

    let signed64 = Fixture::new().expression(&[], |_| Val::<I64>::from(0).add(-1));
    assert_eq!(signed64.instantiate().call::<i64>(()).unwrap(), -1);

    let unsigned64 = Fixture::new().expression(&[], |_| Val::<I64>::from(0).add(u32::MAX));
    assert_eq!(
        unsigned64.instantiate().call::<i64>(()).unwrap(),
        4294967295
    );
}

#[test]
fn constant_additions_wrap_at_the_carrier_width() {
    let dword = Fixture::new().expression(&[], |_| Val::<I32>::from(0x7fff_ffff).add(1));
    assert_eq!(dword.instantiate().call::<i32>(()).unwrap(), i32::MIN);

    let qword = Fixture::new().expression(&[], |_| Val::<I64>::from(u64::MAX).add(1));
    assert_eq!(qword.instantiate().call::<i64>(()).unwrap(), 0);
}

#[test]
fn standalone_expressions_can_be_reused_in_different_function_bodies() {
    let seven = Val::<I32>::from(5).add(2);
    let constant = Fixture::new().expression(&[], |_| Val::from(&seven));
    assert_eq!(constant.instantiate().call::<i32>(()).unwrap(), 7);

    let difference = Fixture::new().expression(&[Type::I32], |body| {
        seven.sub(body.parameter::<I32>(0).unwrap())
    });
    let mut instance = difference.instantiate();
    for (input, expected) in [(3, 4), (-2, 9), (i32::MIN, -2147483641)] {
        assert_eq!(instance.call::<i32>(input).unwrap(), expected);
    }
}

#[test]
fn dword_addition_wraps_and_accepts_signed_operands() {
    let module = Fixture::new().function(&[Type::I32; 2], Some(Type::I32), |body| {
        let left = body.parameter::<I32>(0)?;
        let right = body.parameter::<I32>(1)?;
        body.return_(left.add(right))
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [
        ((i32::MAX, 1), i32::MIN),
        ((i32::MIN, -1), i32::MAX),
        ((-1, 1), 0),
        ((19, -7), 12),
    ] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn qword_addition_wraps_and_accepts_signed_operands() {
    let module = Fixture::new().function(&[Type::I64; 2], Some(Type::I64), |body| {
        let left = body.parameter::<I64>(0)?;
        let right = body.parameter::<I64>(1)?;
        body.return_(left.add(right))
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [
        ((i64::MAX, 1_i64), i64::MIN),
        ((i64::MIN, -1), i64::MAX),
        ((-1, 1), 0),
        ((19, -7), 12),
    ] {
        assert_eq!(
            instance.call::<i64>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn narrow_literals_keep_only_their_logical_bits() {
    fn check<T: IntType>(bits: u32, expected: i32, all_bits: i32) {
        let positive = Fixture::new().expression(&[], |_| Val::<T>::from(bits));
        assert_eq!(positive.instantiate().call::<i32>(()).unwrap(), expected);
        let negative = Fixture::new().expression(&[], |_| Val::<T>::from(-1));
        assert_eq!(negative.instantiate().call::<i32>(()).unwrap(), all_bits);
    }
    check::<I1>(2, 0, 1);
    check::<I8>(0x180, 128, 255);
    check::<I16>(0x18000, 32768, 65535);
}

#[test]
fn boolean_addition_uses_its_logical_bit() {
    let module = Fixture::new().expression(&[], |_| Val::<I1>::from(true).add(true));
    assert_eq!(module.instantiate().call::<i32>(()).unwrap(), 0);
}

#[test]
fn narrow_parameters_preserve_canonical_arguments() {
    fn check<T: IntType>(arguments: &[i32]) {
        let module = Fixture::new().expression(&[T::TYPE], |body| body.parameter::<T>(0).unwrap());
        let mut instance = module.instantiate();
        for &argument in arguments {
            assert_eq!(instance.call::<i32>((argument,)).unwrap(), argument);
        }
    }
    check::<I1>(&[0, 1]);
    check::<I8>(&[255]);
    check::<I16>(&[65535]);
}

#[test]
fn narrow_addition_wraps_at_the_logical_width() {
    fn check<T: IntType>(cases: &[((i32, i32), i32)]) {
        let module = Fixture::new().function(&[T::TYPE; 2], Some(T::TYPE), |body| {
            let left = body.parameter::<T>(0)?;
            let right = body.parameter::<T>(1)?;
            body.return_(left.add(right))
        });
        let mut instance = module.instantiate();
        for &(arguments, expected) in cases {
            assert_eq!(
                instance.call::<i32>(arguments).unwrap(),
                expected,
                "{arguments:?}"
            );
        }
    }
    check::<I1>(&[((0, 1), 1), ((1, 1), 0)]);
    check::<I8>(&[((255, 1), 0), ((127, 1), 128), ((255, 255), 254)]);
    check::<I16>(&[
        ((65535, 1), 0),
        ((32767, 1), 32768),
        ((65535, 65535), 65534),
    ]);
}

#[test]
fn shared_narrow_intermediates_keep_raw_bits_until_return() {
    fn check<T: IntType>(cases: &[(i32, i32)]) {
        let module = Fixture::new().expression(&[T::TYPE], |body| {
            let a = body.parameter::<T>(0).unwrap().add(1);
            let b = a.add(1);
            b.add(&b)
        });
        let mut instance = module.instantiate();
        for &(argument, expected) in cases {
            assert_eq!(
                instance.call::<i32>((argument,)).unwrap(),
                expected,
                "{argument}"
            );
        }
    }
    check::<I1>(&[(1, 0)]);
    check::<I8>(&[(254, 0), (255, 2)]);
    check::<I16>(&[(65534, 0), (65535, 2)]);
}

#[test]
fn shared_dword_intermediates_wrap_without_changing_their_uses() {
    let module = Fixture::new().expression(&[Type::I32], |body| {
        let child = body.parameter::<I32>(0).unwrap().add(1);
        let shared = child.add(2);
        shared.add(&shared)
    });
    let mut instance = module.instantiate();
    for (argument, expected) in [(4, 14), (i32::MAX, 4)] {
        assert_eq!(instance.call::<i32>((argument,)).unwrap(), expected);
    }
}

#[test]
fn shared_qword_intermediates_wrap_without_changing_their_uses() {
    let module = Fixture::new().expression(&[Type::I64], |body| {
        let child = body.parameter::<I64>(0).unwrap().add(1);
        let shared = child.add(2);
        shared.add(&shared)
    });
    let mut instance = module.instantiate();
    for (argument, expected) in [(4_i64, 14), (i64::MAX, 4)] {
        assert_eq!(instance.call::<i64>((argument,)).unwrap(), expected);
    }
}

#[test]
fn overlapping_and_disjoint_subexpressions_preserve_dword_results() {
    for overlap in [true, false] {
        let module = Fixture::new().function(&[Type::I32; 2], Some(Type::I32), |body| {
            let a = body.parameter::<I32>(0)?.add(1);
            let b = body.parameter::<I32>(1)?.add(2);
            let _dead = a.add(9);
            let (left, right) = if overlap {
                (a.add(&b), b.add(&a))
            } else {
                (a.add(&a), b.add(&b))
            };
            body.return_(left.add(right))
        });
        assert_eq!(module.instantiate().call::<i32>((4, 10)).unwrap(), 34);
    }
}

#[test]
fn overlapping_and_disjoint_subexpressions_preserve_qword_results() {
    for overlap in [true, false] {
        let module = Fixture::new().function(&[Type::I64; 2], Some(Type::I64), |body| {
            let a = body.parameter::<I64>(0)?.add(1);
            let b = body.parameter::<I64>(1)?.add(2);
            let _dead = a.add(9);
            let (left, right) = if overlap {
                (a.add(&b), b.add(&a))
            } else {
                (a.add(&a), b.add(&b))
            };
            body.return_(left.add(right))
        });
        assert_eq!(
            module.instantiate().call::<i64>((4_i64, 10_i64)).unwrap(),
            34
        );
    }
}

#[test]
fn mixed_parameter_carriers_preserve_position_and_bits() {
    let parameters = [Type::I32, Type::I64, Type::I32, Type::I64];
    let arguments = (17_i32, i64::MIN, -91_i32, i64::MAX);
    for (index, expected) in [(0, 17), (2, -91)] {
        let module =
            Fixture::new().expression(&parameters, |body| body.parameter::<I32>(index).unwrap());
        assert_eq!(
            module.instantiate().call::<i32>(arguments).unwrap(),
            expected
        );
    }
    for (index, expected) in [(1, i64::MIN), (3, i64::MAX)] {
        let module =
            Fixture::new().expression(&parameters, |body| body.parameter::<I64>(index).unwrap());
        assert_eq!(
            module.instantiate().call::<i64>(arguments).unwrap(),
            expected
        );
    }
}

#[test]
fn exports_follow_declarations_and_can_alias_the_same_function() {
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        result: Some(Type::I32),
    };
    let first = program.declare(signature.clone());
    let second = program.declare(signature);
    for (function, value) in [(second, 11), (first, 7)] {
        program.define(function).unwrap().return_(value).unwrap();
    }
    program.export("first", first).unwrap();
    program.export("second", second).unwrap();
    program.export("second_alias", second).unwrap();
    let module = TestModule::new(&program.compile().unwrap());
    let mut instance = module.instantiate();
    for (name, expected) in [("first", 7), ("second", 11), ("second_alias", 11)] {
        assert_eq!(instance.call_export::<i32>(name, ()).unwrap(), expected);
    }
}
