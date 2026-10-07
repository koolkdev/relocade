use crate::{
    BuildError, IntType, MemoryImport, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};

#[test]
fn bitwise_identities_reuse_existing_values() {
    fn check<T: IntType>() {
        let mut program = Program::new();
        program
            .function(
                Signature {
                    parameters: vec![T::TYPE, T::TYPE],
                    results: vec![T::TYPE],
                },
                |body| {
                    let a = body.parameter::<T>(0)?;
                    let b = body.parameter::<T>(1)?;
                    assert!(a.or(&a).same_expression(&a));
                    assert!(a.and(&a).same_expression(&a));
                    assert!(a.xor(&a).same_expression(&body.value::<T>(0)?));
                    let union = a.or(&b);
                    let intersection = a.and(&b);
                    for operand in [&a, &b] {
                        assert!(union.or(operand).same_expression(&union));
                        assert!(operand.or(&union).same_expression(&union));
                        assert!(intersection.and(operand).same_expression(&intersection));
                        assert!(operand.and(&intersection).same_expression(&intersection));
                    }
                    for (first, second) in [(&a, &b), (&b, &a)] {
                        let toggled = first.xor(second);
                        assert!(toggled.xor(first).same_expression(second));
                        assert!(first.xor(&toggled).same_expression(second));
                        assert!(toggled.xor(second).same_expression(first));
                        assert!(second.xor(&toggled).same_expression(first));
                    }
                    let combined = a.xor(0x66);
                    for toggled in [a.xor(0x55), Val::<T>::from(0x55).xor(&a)] {
                        assert!(toggled.xor(0x33).same_expression(&combined));
                        assert!(Val::<T>::from(0x33)
                            .xor(&toggled)
                            .same_expression(&combined));
                    }
                    assert!(!union.and(&a).same_expression(&union));
                    assert!(!intersection.or(&b).same_expression(&intersection));
                    let sparse_mask = a.and(0x55);
                    assert!(sparse_mask.and(0x55).same_expression(&sparse_mask));
                    body.return_(union)
                },
            )
            .unwrap();
        program.compile().unwrap();
    }
    check::<I1>();
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn masks_preserving_every_possible_bit_share_the_original_value() {
    fn check<T: IntType>() {
        let mut program = Program::new();
        program
            .function(
                Signature {
                    parameters: vec![T::TYPE],
                    results: vec![T::TYPE],
                },
                |body| {
                    let input = body.parameter::<T>(0)?;
                    let low = input.and(7);
                    for mask in [7, 15, 127] {
                        assert!(low.and(mask).same_expression(&low));
                        assert!(Val::<T>::from(mask).and(&low).same_expression(&low));
                    }
                    // These masks can discard bit two or bit one, respectively.
                    assert!(!low.and(3).same_expression(&low));
                    assert!(!low.and(5).same_expression(&low));
                    body.return_(low)
                },
            )
            .unwrap();
        program.compile().unwrap();
    }
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn mask_folding_respects_carries_and_signed_upper_bits() {
    let mut program = Program::new();
    program
        .function(
            Signature {
                parameters: vec![Type::I8],
                results: vec![Type::I32],
            },
            |body| {
                let byte = body.parameter::<I8>(0)?;
                let signed = byte.signed().extend::<I32>();
                assert!(!signed.and(255).same_expression(&signed));
                let low = byte.unsigned().extend::<I32>().and(7);
                let carried = low.add(1);
                assert!(!carried.and(7).same_expression(&carried));
                assert!(carried.and(15).same_expression(&carried));
                body.return_(carried)
            },
        )
        .unwrap();
    program.compile().unwrap();
}

#[test]
fn offsets_combine_across_masks_preserving_the_observed_low_bits() {
    let mut program = Program::new();
    program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I32],
            },
            |body| {
                let input = body.parameter::<I32>(0)?;
                let low = input.and(7);
                let pushed = low.add(7).and(7).truncate::<I8>();
                let popped = pushed.unsigned().extend::<I32>().add(1).and(7);
                assert!(popped.same_expression(&low));
                let result = input.add(5).and(15).add(6).and(7);
                assert!(result.same_expression(&input.add(3).and(7)));
                let sparse = input.add(5).and(5).add(6).and(7);
                assert!(!sparse.same_expression(&result));
                body.return_(popped)
            },
        )
        .unwrap();
    program.compile().unwrap();
}

#[test]
fn low_masks_discard_only_disjoint_bitwise_inputs() {
    fn check<T: IntType>() {
        let mut program = Program::new();
        program
            .function(
                Signature {
                    parameters: vec![T::TYPE, T::TYPE],
                    results: vec![T::TYPE],
                },
                |body| {
                    let input = body.parameter::<T>(0)?;
                    let other = body.parameter::<T>(1)?;
                    let high = other.and(0x80);
                    let expected = input.add(5).and(0x7f);
                    for combined in [
                        input.or(&high),
                        high.or(&input),
                        input.xor(&high),
                        high.xor(&input),
                    ] {
                        assert!(combined.add(5).and(0x7f).same_expression(&expected));
                    }
                    let overlapping = other.and(0xc0);
                    assert!(!input
                        .or(&overlapping)
                        .and(0x7f)
                        .same_expression(&input.and(0x7f)));
                    assert!(!input
                        .xor(overlapping)
                        .and(0x7f)
                        .same_expression(&input.and(0x7f)));
                    body.return_(expected)
                },
            )
            .unwrap();
        program.compile().unwrap();
    }
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn long_mask_chains_preserve_low_bit_sums() {
    let mut program = Program::new();
    program
        .function(
            Signature {
                parameters: vec![Type::I32],
                results: vec![Type::I32],
            },
            |body| {
                let input = body.parameter::<I32>(0)?;
                let mut value = input.clone();
                for _ in 0..20_000 {
                    value = value.add(2).and(0x17);
                }
                let result = value.add(1).and(7);
                assert!(result.same_expression(&input.add(1).and(7)));
                body.return_(result)
            },
        )
        .unwrap();
    program.compile().unwrap();
}

#[test]
fn equivalent_constant_offsets_share_values_at_each_logical_width() {
    fn check<T: IntType>() {
        let mut program = Program::new();
        let function = program.declare(Signature {
            parameters: vec![T::TYPE],
            results: vec![T::TYPE],
        });
        program
            .define(function, |body| {
                let input = body.parameter::<T>(0).unwrap();
                let minus_four = Val::<T>::from(0).sub(4);
                let highest_bit = Val::<T>::literal(1u64 << (T::TYPE.bits() - 1));
                for (actual, expected) in [
                    (input.sub(4), input.add(minus_four)),
                    (Val::<T>::from(7).add(input.sub(3)), input.add(4)),
                    (input.add(2).add(3).sub(5), input.clone()),
                    (
                        input.add(Val::<T>::literal(T::TYPE.mask())).add(1),
                        input.clone(),
                    ),
                    (input.sub(&highest_bit), input.add(highest_bit)),
                ] {
                    assert!(actual.same_expression(&expected), "{:?}", T::TYPE);
                }
                body.return_(input)
            })
            .unwrap();
        program.compile().unwrap();
    }
    check::<I1>();
    check::<I8>();
    check::<I16>();
    check::<I32>();
    check::<I64>();
}

#[test]
fn cancelling_offsets_retain_the_original_operand_scope() {
    let mut program = Program::new();
    let memory = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "memory".into(),
        minimum: 1,
        maximum: None,
        shared: false,
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I32],
        results: vec![Type::I32],
    });
    program
        .define(function, |mut body| {
            let input = body.parameter::<I32>(0).unwrap();
            let mut retained = None;
            body.if_(false, |mut child| {
                let offset = child.load::<I32>(memory, 0)?.and(0).add(4);
                let result = input.add(offset).sub(4);
                assert!(result.same_expression(&input));
                retained = Some(child.value(result)?);
                Ok(())
            })
            .unwrap();
            assert_eq!(
                body.value(retained.unwrap()).err(),
                Some(BuildError::OutOfScope),
            );
            body.return_(input)
        })
        .unwrap();
    program.compile().unwrap();
}
