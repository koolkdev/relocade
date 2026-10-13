//! Identities fold runtime operands without introducing scalar fact analysis.

use super::*;
use wasm86_compiler::V64;

fn identities(v8: bool) {
    let mut initial = vec![0xa5; 352];
    initial[..16].copy_from_slice(&LEFT.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &initial);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let x = body.load::<V128>(memory, 0)?;
        let count = body.parameter::<I32>(0)?;
        let results = [
            x.add_lanes::<I8>(0_u128),
            x.sub_lanes::<I32>(&x),
            x.eq_lanes::<I64>(&x),
            x.signed_gt_lanes::<I16>(&x),
            x.signed_saturating_add_lanes::<I16>(0_u128),
            x.unsigned_saturating_sub_lanes::<I8>(&x),
            x.mul_lanes::<I32>(0_u128),
            x.mul_lanes::<I16>(0x0001_0001_0001_0001_0001_0001_0001_0001_u128),
            x.signed_dot_i16_pairs(0_u128),
            x.shl_lanes::<I16>(48),
            Val::<V128>::from(0_u128).signed_shr_lanes::<I32>(count),
        ];
        for (i, result) in results.into_iter().enumerate() {
            body.store(memory, 16 + 16 * i as u32, result)?;
        }
        let low = x.low_half();
        for (i, result) in [
            low.add_lanes::<I16>(0_u64),
            low.sub_lanes::<I8>(&low),
            low.eq_lanes::<I32>(&low),
            low.signed_saturating_sub_lanes::<I16>(0_u64),
            Val::<V64>::from(0_u64).mul_lanes::<I64>(&low),
            low.mul_lanes::<I16>(0x0001_0001_0001_0001_u64),
            Val::<V64>::from(0x0000_0001_0000_0001_u64).mul_lanes::<I32>(&low),
        ]
        .into_iter()
        .enumerate()
        {
            body.store(memory, 192 + 16 * i as u32, result.zero_extend())?;
        }
        for (i, result) in [
            low.signed_widening_mul_lanes::<I8>(0_u64),
            x.signed_widening_mul_low_lanes::<I16>(0_u128),
            x.signed_widening_mul_high_lanes::<I32>(0_u128),
        ]
        .into_iter()
        .enumerate()
        {
            body.store(memory, 304 + 16 * i as u32, result)?;
        }
        body.return_(())
    });
    let mut expected = initial.clone();
    for (i, bits) in [
        LEFT,
        0,
        u128::MAX,
        0,
        LEFT,
        0,
        0,
        LEFT,
        0,
        LEFT,
        0,
        LEFT & u128::from(u64::MAX),
        0,
        u128::from(u64::MAX),
        LEFT & u128::from(u64::MAX),
        0,
        LEFT & u128::from(u64::MAX),
        LEFT & u128::from(u64::MAX),
        0,
        0,
        0,
    ]
    .into_iter()
    .enumerate()
    {
        expected[16 + 16 * i..32 + 16 * i].copy_from_slice(&bits.to_le_bytes());
    }
    // Every packed calculation is an identity, including the dynamic shift of zero.
    for payload in wasmparser::Parser::new(0).parse_all(module.bytes()) {
        if let wasmparser::Payload::CodeSectionEntry(body) = payload.unwrap() {
            for op in body.get_operators_reader().unwrap() {
                use wasmparser::Operator::*;
                assert!(!matches!(
                    op.unwrap(),
                    I8x16Add
                        | I32x4Sub
                        | I64x2Eq
                        | I16x8GtS
                        | I16x8AddSatS
                        | I8x16SubSatU
                        | I32x4Mul
                        | I16x8Mul
                        | I32x4DotI16x8S
                        | I16x8Shl
                        | I32x4ShrS
                        | I16x8Add
                        | I8x16Sub
                        | I32x4Eq
                        | I16x8SubSatS
                        | I64x2Mul
                        | I16x8ExtMulLowI8x16S
                        | I32x4ExtMulLowI16x8S
                        | I64x2ExtMulHighI32x4S
                ));
            }
        }
    }
    for count in [0, 1, 31, -1] {
        check(&module, &initial, &expected, count, v8);
    }
}

#[test]
fn packed_identities_remove_calculations_and_preserve_both_vector_widths() {
    identities(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_packed_identities_remove_calculations_and_preserve_both_vector_widths() {
    identities(true);
}
