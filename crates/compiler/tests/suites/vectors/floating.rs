//! Floating lane views preserve strict arithmetic across runtime and literal inputs.

use super::*;
use wasm86_compiler::I64;

#[path = "floating/conversions.rs"]
mod conversions;
#[path = "floating/encoding.rs"]
mod encoding;

fn words(lanes: [u32; 4]) -> u128 {
    lanes.into_iter().enumerate().fold(0, |bits, (lane, word)| {
        bits | (u128::from(word) << (lane * 32))
    })
}

fn qwords(lanes: [u64; 2]) -> u128 {
    u128::from(lanes[0]) | (u128::from(lanes[1]) << 64)
}

fn case(
    left: u128,
    right: u128,
    expected: &[u128],
    calculate: impl Fn(&Val<V128>, &Val<V128>) -> Vec<Val<V128>>,
) {
    let mut bytes = vec![0xa5; 32 + 3 * expected.len() * 16];
    bytes[..16].copy_from_slice(&left.to_le_bytes());
    bytes[16..32].copy_from_slice(&right.to_le_bytes());
    let mut fixture = Fixture::new();
    let memory = fixture.memory("state", &bytes);
    let module = fixture.function(&[Type::I32], &[], |mut body| {
        let runtime = (
            body.load::<V128>(memory, 0)?,
            body.load::<V128>(memory, 16)?,
        );
        let literals = (Val::from(left), Val::from(right));
        let admitted = (
            body.value(literals.0.clone())?,
            body.value(literals.1.clone())?,
        );
        for (group, (left, right)) in [runtime, literals, admitted].into_iter().enumerate() {
            let results = calculate(&left, &right);
            assert_eq!(results.len(), expected.len());
            for (index, result) in results.into_iter().enumerate() {
                body.store(
                    memory,
                    (32 + (group * expected.len() + index) * 16) as u32,
                    result,
                )?;
            }
        }
        body.return_(())
    });
    let mut wanted = bytes.clone();
    for group in 0..3 {
        for (index, bits) in expected.iter().enumerate() {
            let offset = 32 + (group * expected.len() + index) * 16;
            wanted[offset..offset + 16].copy_from_slice(&bits.to_le_bytes());
        }
    }
    check(&module, &wanted, 0);
}

// Wasm permits arithmetic NaN payload choices. Normalize only those results;
// transport, comparisons and sign-bit operations are checked bit for bit.
fn canonical_nan(mut vector: Val<V128>, wide: bool) -> Val<V128> {
    if wide {
        for lane in 0..2 {
            let bits = vector.extract_lane::<I64>(lane);
            let nan = bits
                .and(0x7fff_ffff_ffff_ffff_u64)
                .unsigned()
                .ge(0x7ff0_0000_0000_0001_u64);
            vector = vector.replace_lane(lane, nan.select(0x7ff8_0000_0000_0000_u64, bits));
        }
    } else {
        for lane in 0..4 {
            let bits = vector.extract_lane::<I32>(lane);
            let nan = bits.and(0x7fff_ffff_u32).unsigned().ge(0x7f80_0001_u32);
            vector = vector.replace_lane(lane, nan.select(0x7fc0_0000_u32, bits));
        }
    }
    vector
}

fn arithmetic() {
    case(
        words([0x3fc0_0000, 0x8000_0000, 0x0080_0000, 0x3f80_0000]),
        words([0x4000_0000, 0x8000_0000, 0x3f00_0000, 0x3380_0000]),
        &[
            words([0x4060_0000, 0x8000_0000, 0x3f00_0000, 0x3f80_0000]),
            words([0xbf00_0000, 0, 0xbf00_0000, 0x3f7f_ffff]),
            words([0x4040_0000, 0, 0x0040_0000, 0x3380_0000]),
            words([0x3f40_0000, 0x7fc0_0000, 0x0100_0000, 0x4b80_0000]),
        ],
        |a, b| {
            [
                a.f32x4().add(b),
                a.f32x4().sub(b),
                a.f32x4().mul(b),
                a.f32x4().div(b),
            ]
            .map(|v| canonical_nan(v, false))
            .into()
        },
    );
    for (a, b, expected) in [
        (
            [0x3ff8_0000_0000_0000, 0x8000_0000_0000_0000],
            [0x4000_0000_0000_0000, 0x8000_0000_0000_0000],
            [
                [0x400c_0000_0000_0000, 0x8000_0000_0000_0000],
                [0xbfe0_0000_0000_0000, 0],
                [0x4008_0000_0000_0000, 0],
                [0x3fe8_0000_0000_0000, 0x7ff8_0000_0000_0000],
            ],
        ),
        (
            [0x0010_0000_0000_0000, 0x3ff0_0000_0000_0000],
            [0x3fe0_0000_0000_0000, 0x3ca0_0000_0000_0000],
            [
                [0x3fe0_0000_0000_0000, 0x3ff0_0000_0000_0000],
                [0xbfe0_0000_0000_0000, 0x3fef_ffff_ffff_ffff],
                [0x0008_0000_0000_0000, 0x3ca0_0000_0000_0000],
                [0x0020_0000_0000_0000, 0x4340_0000_0000_0000],
            ],
        ),
    ] {
        case(qwords(a), qwords(b), &expected.map(qwords), |a, b| {
            [
                a.f64x2().add(b),
                a.f64x2().sub(b),
                a.f64x2().mul(b),
                a.f64x2().div(b),
            ]
            .map(|v| canonical_nan(v, true))
            .into()
        });
    }
}

fn rounding() {
    case(
        words([0x4020_0000, 0xc020_0000, 0xbe80_0000, 0x7f80_0042]),
        0,
        &[
            words([0x4040_0000, 0xc000_0000, 0x8000_0000, 0x7fc0_0000]),
            words([0x4000_0000, 0xc040_0000, 0xbf80_0000, 0x7fc0_0000]),
            words([0x4000_0000, 0xc000_0000, 0x8000_0000, 0x7fc0_0000]),
            words([0x4000_0000, 0xc000_0000, 0x8000_0000, 0x7fc0_0000]),
        ],
        |a, _| {
            [
                a.f32x4().ceil(),
                a.f32x4().floor(),
                a.f32x4().trunc(),
                a.f32x4().nearest(),
            ]
            .map(|v| canonical_nan(v, false))
            .into()
        },
    );
    case(
        words([0x4080_0000, 0x4000_0000, 0x8000_0000, 0xbf80_0000]),
        0,
        &[words([0x4000_0000, 0x3fb5_04f3, 0x8000_0000, 0x7fc0_0000])],
        |a, _| vec![canonical_nan(a.f32x4().sqrt(), false)],
    );
    case(
        qwords([0x4004_0000_0000_0000, 0xbfd0_0000_0000_0000]),
        0,
        &[
            qwords([0x4008_0000_0000_0000, 0x8000_0000_0000_0000]),
            qwords([0x4000_0000_0000_0000, 0xbff0_0000_0000_0000]),
            qwords([0x4000_0000_0000_0000, 0x8000_0000_0000_0000]),
            qwords([0x4000_0000_0000_0000, 0x8000_0000_0000_0000]),
        ],
        |a, _| {
            vec![
                a.f64x2().ceil(),
                a.f64x2().floor(),
                a.f64x2().trunc(),
                a.f64x2().nearest(),
            ]
        },
    );
    case(
        qwords([0x4000_0000_0000_0000, 0x8000_0000_0000_0000]),
        0,
        &[qwords([0x3ff6_a09e_667f_3bcd, 0x8000_0000_0000_0000])],
        |a, _| vec![a.f64x2().sqrt()],
    );
}

fn comparisons_and_signs() {
    let yes = u32::MAX;
    case(
        words([0, 0x7f80_0042, 0xc000_0000, 0x7f80_0000]),
        words([0x8000_0000, 0x3f80_0000, 0xbf80_0000, 0x7f80_0000]),
        &[
            words([yes, 0, 0, yes]),
            words([0, yes, yes, 0]),
            words([0, 0, yes, 0]),
            words([yes, 0, yes, yes]),
            0,
            words([yes, 0, 0, yes]),
            words([yes, 0, yes, yes]),
        ],
        |a, b| {
            vec![
                a.f32x4().eq(b),
                a.f32x4().ne(b),
                a.f32x4().lt(b),
                a.f32x4().le(b),
                a.f32x4().gt(b),
                a.f32x4().ge(b),
                a.f32x4().eq(a),
            ]
        },
    );
    case(
        qwords([0, 0x7ff0_0000_0000_0042]),
        qwords([0x8000_0000_0000_0000, 0x3ff0_0000_0000_0000]),
        &[
            qwords([u64::MAX, 0]),
            qwords([0, u64::MAX]),
            0,
            qwords([u64::MAX, 0]),
            0,
            qwords([u64::MAX, 0]),
            qwords([u64::MAX, 0]),
        ],
        |a, b| {
            vec![
                a.f64x2().eq(b),
                a.f64x2().ne(b),
                a.f64x2().lt(b),
                a.f64x2().le(b),
                a.f64x2().gt(b),
                a.f64x2().ge(b),
                a.f64x2().eq(a),
            ]
        },
    );
    case(
        words([0xff80_0042, 0x7fc0_0042, 0x8000_0000, 0xbf80_0000]),
        0,
        &[
            words([0x7f80_0042, 0x7fc0_0042, 0, 0x3f80_0000]),
            words([0x7f80_0042, 0xffc0_0042, 0, 0x3f80_0000]),
        ],
        |a, _| vec![a.f32x4().abs(), a.f32x4().neg()],
    );
    case(
        qwords([0xfff0_0000_0000_0042, 0x8000_0000_0000_0000]),
        0,
        &[
            qwords([0x7ff0_0000_0000_0042, 0]),
            qwords([0x7ff0_0000_0000_0042, 0]),
        ],
        |a, _| vec![a.f64x2().abs(), a.f64x2().neg()],
    );
}

#[test]
fn floating_vector_arithmetic_rounding_and_masks() {
    arithmetic();
    rounding();
    comparisons_and_signs();
}
