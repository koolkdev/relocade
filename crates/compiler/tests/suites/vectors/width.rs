use super::*;
use relocade_compiler::{I16, I64, V64};

#[test]
fn logical_vector_width_controls_layout_and_literal_evaluation() {
    let mut initial = vec![0xa5; 400];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let full = body.load::<V128>(memory, 0)?;
        let low = full.low_half();
        let other = body.load::<V64>(memory, 16)?;
        let operations = |a: Val<V64>, b: Val<V64>| {
            [
                a.add_lanes::<I16>(&b),
                a.eq_lanes::<I16>(&a),
                a.signed_narrow_lanes::<I8>(&b),
                a.unsigned_narrow_lanes::<I8>(&b),
                a.shuffle_bytes(&b, [0, 8, 1, 9, 7, 15, 6, 14]),
                a.shl_lanes::<I16>(3),
                a.replace_lane::<I16>(3, 0x1234),
            ]
        };
        for (index, result) in operations(low.clone(), other.clone())
            .into_iter()
            .chain(operations(
                Val::<V64>::from(LEFT as u64),
                Val::<V64>::from(RIGHT as u64),
            ))
            .enumerate()
        {
            body.store(memory, 32 + index as u32 * 8, result)?;
        }
        body.store(memory, 144, low.zero_extend())?;
        body.store(memory, 160, full)?;
        body.store(memory, 176, low.concat(&other))?;
        body.store(memory, 192, low.signed_widening_mul_lanes::<I16>(&other))?;
        body.store(memory, 208, low.to_bits())?;
        let roundtrip = Val::<V64>::from_bits(low.to_bits());
        assert!(roundtrip.same_expression(&low));
        body.store(memory, 216, roundtrip.zero_extend())?;
        body.return_(())
    });
    let left = (LEFT as u64).to_le_bytes();
    let right = (RIGHT as u64).to_le_bytes();
    let words = |bytes: [u8; 8]| {
        std::array::from_fn::<_, 4, _>(|i| i16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]))
    };
    let a = words(left);
    let b = words(right);
    let sum: Vec<_> = a
        .iter()
        .zip(b)
        .flat_map(|(&x, y)| x.wrapping_add(y).to_le_bytes())
        .collect();
    let narrow = |signed| {
        a.iter()
            .chain(b.iter())
            .map(|&x| {
                if signed {
                    x.clamp(-128, 127) as u8
                } else {
                    x.clamp(0, 255) as u8
                }
            })
            .collect::<Vec<_>>()
    };
    let shifted: Vec<_> = a
        .iter()
        .flat_map(|&x| (x as u16).wrapping_shl(3).to_le_bytes())
        .collect();
    let mut replaced = left;
    replaced[6..].copy_from_slice(&0x1234_u16.to_le_bytes());
    let results = [
        sum,
        vec![255; 8],
        narrow(true),
        narrow(false),
        vec![
            left[0], right[0], left[1], right[1], left[7], right[7], left[6], right[6],
        ],
        shifted,
        replaced.to_vec(),
    ];
    let mut expected = initial.clone();
    for (i, result) in results.iter().cycle().take(14).enumerate() {
        expected[32 + i * 8..40 + i * 8].copy_from_slice(result);
    }
    expected[144..160].copy_from_slice(&u128::from(LEFT as u64).to_le_bytes());
    expected[160..176].copy_from_slice(&LEFT.to_le_bytes());
    expected[176..184].copy_from_slice(&left);
    expected[184..192].copy_from_slice(&right);
    for i in 0..4 {
        expected[192 + i * 4..196 + i * 4]
            .copy_from_slice(&(i32::from(a[i]) * i32::from(b[i])).to_le_bytes());
    }
    expected[208..216].copy_from_slice(&left);
    expected[216..232].copy_from_slice(&u128::from(LEFT as u64).to_le_bytes());
    check(&module, &expected, 0);
}

#[test]
fn vector_width_survives_calls_and_joins_with_dirty_carriers() {
    let mut initial = vec![0xa5; 112];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    initial[16..32].copy_from_slice(&RIGHT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let helper = fixture
        .program
        .function(signature(&[Type::V64], &[Type::V64, Type::V128]), |body| {
            let input = body.parameter::<V64>(0)?;
            body.return_((input.eq_lanes::<I16>(&input), input.zero_extend()))
        })
        .unwrap();
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let left = body.load::<V128>(memory, 0)?.low_half();
        let right = body.load::<V128>(memory, 16)?.low_half();
        let (called, widened_argument) =
            body.call::<(V64, V128)>(helper, &[left.clone().into()])?;
        let (first, second) = body.loop_::<(I32, V64, V64), (V64, V64)>(
            (&count, left.clone(), right.clone()),
            |mut iteration, labels, (remaining, first, second)| {
                iteration.branch_if(remaining.eq(0), &labels.exit, (&first, &second))?;
                iteration.branch(&labels.again, (remaining.sub(1), second, first))
            },
        )?;
        let joined =
            body.if_value::<V64>(count.eq(0), |arm| arm.yield_(left), |arm| arm.yield_(right))?;
        for (offset, value) in [(32, first), (48, second), (64, called), (80, joined)] {
            body.store(memory, offset, value.zero_extend())?;
        }
        body.store(memory, 96, widened_argument)?;
        body.return_(())
    });
    for count in [0, 1, 2, 5] {
        let pair = if count % 2 == 0 {
            [LEFT, RIGHT]
        } else {
            [RIGHT, LEFT]
        };
        let results = [
            pair[0] as u64,
            pair[1] as u64,
            u64::MAX,
            (if count == 0 { LEFT } else { RIGHT }) as u64,
        ];
        let mut expected = initial.clone();
        for (index, bits) in results.into_iter().enumerate() {
            expected[32 + index * 16..48 + index * 16]
                .copy_from_slice(&u128::from(bits).to_le_bytes());
        }
        expected[96..112].copy_from_slice(&u128::from(LEFT as u64).to_le_bytes());
        check(&module, &expected, count);
    }
}

#[test]
fn vector_returns_zero_the_unused_carrier_half() {
    let module = Fixture::new().function(&[Type::I64], &[Type::V64], |body| {
        let bits = body.parameter::<I64>(0)?;
        body.return_(Val::<V64>::from_bits(bits))
    });
    for bits in [0_u64, 1, u64::MAX, 0x8000_0000_0000_0000] {
        let actual: wasmtime::V128 = module.instantiate().call((bits as i64,)).unwrap();
        assert_eq!(actual.as_u128(), u128::from(bits));
    }
}

#[test]
fn vector_memory_accesses_use_exactly_eight_bytes() {
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &[0; 8]);
    let module = fixture.function(&[], &[Type::I64], |mut body| {
        body.store(memory, 65528, Val::<V64>::from(0x12345678_90abcdef_u64))?;
        let bits = body.load::<V64>(memory, 65528)?.to_bits();
        body.return_(bits)
    });
    let actual: i64 = module.instantiate().call(()).unwrap();
    assert_eq!(actual as u64, 0x12345678_90abcdef);
}

#[test]
fn vector_signatures_admit_native_literals_at_their_logical_width() {
    let bits = 0x12345678_90abcdef_u64;
    let module = Fixture::new().function(&[], &[Type::V64; 2], |body| {
        body.return_((bits, bits.to_le_bytes()))
    });
    let actual: (wasmtime::V128, wasmtime::V128) = module.instantiate().call(()).unwrap();
    assert_eq!(actual.0.as_u128(), u128::from(bits));
    assert_eq!(actual.1.as_u128(), u128::from(bits));

    for (argument, expected, actual) in [
        (Argument::from([0_u8; 8]), Type::I64, Type::V64),
        (Argument::from(0_u128), Type::V64, Type::V128),
    ] {
        let error = Program::new()
            .function(signature(&[], &[expected]), |body| body.return_(argument))
            .err();
        assert_eq!(error, Some(BuildError::TypeMismatch { expected, actual }));
    }
}

#[test]
fn narrow_vector_lane_indices_wrap_at_the_logical_width() {
    let bits = 0x123456789abcdef0_u64;
    assert!(Val::<V64>::from(bits)
        .extract_lane::<I64>(1)
        .same_expression(&Val::<I64>::from(bits)));
}

#[test]
fn narrow_vector_shuffles_zero_out_of_range_indices() {
    assert!(Val::<V64>::from(u64::MAX)
        .shuffle_bytes(u64::MAX, [16; 8])
        .same_expression(&Val::<V64>::from(0_u64)));
}
