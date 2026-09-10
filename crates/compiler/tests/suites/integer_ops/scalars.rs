use crate::fixture::Fixture;
use wasm86_compiler::{AtLeast, IntType, Type, I1, I16, I32, I64, I8};

#[test]
fn dword_masks_preserve_the_selected_bits() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .and(0x00ff_00ffu32)
            .or(0x8000_0000u32)
    });
    assert_eq!(
        module.instantiate().call::<i32>((-2023406815,)).unwrap(),
        -2140864479
    );
}

#[test]
fn qword_masks_preserve_the_high_bit() {
    let module = Fixture::new().expression(&[Type::I64], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .and(0x8000_0000_0000_0000u64)
            .or(1)
    });
    assert_eq!(
        module
            .instantiate()
            .call::<i64>((-9223372036854775808_i64,))
            .unwrap(),
        -9223372036854775807_i64
    );
}

#[test]
fn dword_equality_compares_the_full_carrier() {
    let module = Fixture::new().expression(&[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .eq(b.parameter::<I32>(1).unwrap())
    });
    assert_eq!(module.instantiate().call::<i32>((-1, -1)).unwrap(), 1);
}

#[test]
fn dword_inequality_compares_the_full_carrier() {
    let module = Fixture::new().expression(&[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .ne(b.parameter::<I32>(1).unwrap())
    });
    assert_eq!(module.instantiate().call::<i32>((-1, 0)).unwrap(), 1);
}

#[test]
fn dword_unsigned_less_than_uses_unsigned_order() {
    let module = Fixture::new().expression(&[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .unsigned()
            .lt(b.parameter::<I32>(1).unwrap())
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [((-2147483648, 1), 0), ((1, -2147483648), 1)] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn dword_unsigned_greater_or_equal_uses_unsigned_order() {
    let module = Fixture::new().expression(&[Type::I32; 2], |b| {
        b.parameter::<I32>(0)
            .unwrap()
            .unsigned()
            .ge(b.parameter::<I32>(1).unwrap())
    });
    assert_eq!(
        module.instantiate().call::<i32>((-2147483648, 1)).unwrap(),
        1
    );
}

#[test]
fn qword_equality_compares_the_full_carrier() {
    let module = Fixture::new().expression(&[Type::I64; 2], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .eq(b.parameter::<I64>(1).unwrap())
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [
        ((-9223372036854775808_i64, -9223372036854775808_i64), 1),
        ((-9223372036854775808_i64, 0_i64), 0),
    ] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn qword_unsigned_less_than_uses_unsigned_order() {
    let module = Fixture::new().expression(&[Type::I64; 2], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .unsigned()
            .lt(b.parameter::<I64>(1).unwrap())
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [
        ((-9223372036854775808_i64, 1_i64), 0),
        ((1_i64, -9223372036854775808_i64), 1),
    ] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn qword_unsigned_greater_or_equal_uses_unsigned_order() {
    let module = Fixture::new().expression(&[Type::I64; 2], |b| {
        b.parameter::<I64>(0)
            .unwrap()
            .unsigned()
            .ge(b.parameter::<I64>(1).unwrap())
    });
    assert_eq!(
        module
            .instantiate()
            .call::<i32>((-9223372036854775808_i64, 1_i64))
            .unwrap(),
        1
    );
}

#[test]
fn qword_truncation_keeps_the_low_dword() {
    let module = Fixture::new().expression(&[Type::I64], |b| {
        b.parameter::<I64>(0).unwrap().truncate::<I32>()
    });
    assert_eq!(
        module
            .instantiate()
            .call::<i32>((-9223372036549334067_i64,))
            .unwrap(),
        305441741
    );
}

#[test]
fn byte_truncation_keeps_the_low_byte() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0).unwrap().truncate::<I8>()
    });
    assert_eq!(module.instantiate().call::<i32>((305441741,)).unwrap(), 205);
}

#[test]
fn boolean_truncation_keeps_only_the_low_bit() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0).unwrap().truncate::<I1>()
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [((2,), 0), ((3,), 1)] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn unsigned_dword_extension_clears_the_high_bits() {
    let module = Fixture::new().expression(&[Type::I32], |b| {
        b.parameter::<I32>(0).unwrap().unsigned().extend::<I64>()
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [((-2147483648,), 2147483648_i64), ((-1,), 4294967295_i64)] {
        assert_eq!(
            instance.call::<i64>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn opcode_group_masks_match_all_register_encodings() {
    let module = Fixture::new().expression(&[Type::I8], |b| {
        b.parameter::<I8>(0).unwrap().and(0xf8).eq(0xb8)
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [((184,), 1), ((191,), 1), ((192,), 0)] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn opcode_register_masks_extract_the_low_three_bits() {
    let module = Fixture::new().expression(&[Type::I8], |b| b.parameter::<I8>(0).unwrap().and(7));
    let mut instance = module.instantiate();
    for (arguments, expected) in [((184,), 0), ((191,), 7)] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn byte_assembly_preserves_little_endian_order() {
    let module = Fixture::new().expression(&[Type::I8; 4], |b| {
        let byte0 = b.parameter::<I8>(0).unwrap().unsigned().extend::<I32>();
        let byte1 = b
            .parameter::<I8>(1)
            .unwrap()
            .unsigned()
            .extend::<I32>()
            .shl(8);
        let byte2 = b
            .parameter::<I8>(2)
            .unwrap()
            .unsigned()
            .extend::<I32>()
            .shl(16);
        let byte3 = b
            .parameter::<I8>(3)
            .unwrap()
            .unsigned()
            .extend::<I32>()
            .shl(24);
        byte0.or(&byte1).or(&byte2).or(&byte3)
    });
    let mut instance = module.instantiate();
    for (arguments, expected) in [
        ((120, 86, 52, 18), 305419896),
        ((243, 15, 184, 102), 1723338739),
        ((0, 0, 0, 128), -2147483648),
    ] {
        assert_eq!(
            instance.call::<i32>(arguments).unwrap(),
            expected,
            "{arguments:?}"
        );
    }
}

#[test]
fn narrow_zero_tests_observe_the_wrapped_sum() {
    fn check<T: IntType>(cases: &[(i32, i32)]) {
        let module =
            Fixture::new().expression(&[T::TYPE], |b| b.parameter::<T>(0).unwrap().add(1).eq(0));
        let mut instance = module.instantiate();
        for &(argument, expected) in cases {
            assert_eq!(
                instance.call::<i32>((argument,)).unwrap(),
                expected,
                "{argument}"
            );
        }
    }
    check::<I1>(&[(1, 1)]);
    check::<I8>(&[(255, 1), (127, 0)]);
    check::<I16>(&[(65535, 1)]);
}

#[test]
fn narrow_nonzero_tests_observe_the_wrapped_sum() {
    fn check<T: IntType>(cases: &[(i32, i32)]) {
        let module =
            Fixture::new().expression(&[T::TYPE], |b| b.parameter::<T>(0).unwrap().add(1).ne(0));
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
    check::<I8>(&[(255, 0), (127, 1)]);
    check::<I16>(&[(65535, 0)]);
}

#[test]
fn narrow_unsigned_less_than_observes_the_wrapped_sum() {
    fn check<T: IntType>(cases: &[(i32, i32)]) {
        let module = Fixture::new().expression(&[T::TYPE], |b| {
            b.parameter::<T>(0).unwrap().add(1).unsigned().lt(1)
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
    check::<I1>(&[(1, 1)]);
    check::<I8>(&[(255, 1), (127, 0)]);
    check::<I16>(&[(65535, 1)]);
}

#[test]
fn narrow_unsigned_greater_or_equal_observes_the_wrapped_sum() {
    fn check<T: IntType>(cases: &[(i32, i32)]) {
        let module = Fixture::new().expression(&[T::TYPE], |b| {
            b.parameter::<T>(0).unwrap().add(1).unsigned().ge(1)
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
    check::<I8>(&[(255, 0), (127, 1)]);
    check::<I16>(&[(65535, 0)]);
}

#[test]
fn narrow_unsigned_extension_observes_the_wrapped_sum() {
    fn check<T: IntType>(cases: &[(i32, i32)])
    where
        I32: AtLeast<T>,
    {
        let module = Fixture::new().expression(&[T::TYPE], |b| {
            b.parameter::<T>(0)
                .unwrap()
                .add(1)
                .unsigned()
                .extend::<I32>()
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
    check::<I8>(&[(255, 0), (127, 128)]);
    check::<I16>(&[(65535, 0)]);
}
