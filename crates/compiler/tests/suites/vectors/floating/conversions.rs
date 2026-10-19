//! Conversion boundaries include saturation, ignored lanes and cleared upper lanes.

use super::*;

fn conversions() {
    case(
        words([0x8000_0000, 0x7fff_ffff, 0x0100_0001, 0xfeff_ffff]),
        0,
        &[
            words([0xcf00_0000, 0x4f00_0000, 0x4b80_0000, 0xcb80_0000]),
            qwords([0xc1e0_0000_0000_0000, 0x41df_ffff_ffc0_0000]),
        ],
        |a, _| vec![a.signed_i32_lanes_to_f32(), a.low_signed_i32_lanes_to_f64()],
    );
    case(
        words([0x3f80_0000, 0x8000_0000, 0x7f80_0042, 0x7fc0_0042]),
        0,
        &[qwords([0x3ff0_0000_0000_0000, 0x8000_0000_0000_0000])],
        |a, _| vec![a.f32x4().promote_low()],
    );
    case(
        words([0x7f80_0042, 1, 0, 0]),
        0,
        &[qwords([0x7ff8_0000_0000_0000, 0x36a0_0000_0000_0000])],
        |a, _| vec![canonical_nan(a.f32x4().promote_low(), true)],
    );
    case(
        qwords([0x3ff0_0000_1000_0000, 0x8000_0000_0000_0001]),
        0,
        &[words([0x3f80_0000, 0x8000_0000, 0, 0])],
        |a, _| vec![a.f64x2().demote_zero()],
    );
    case(
        qwords([0x7ff0_0000_0000_0042, 0x7fef_ffff_ffff_ffff]),
        0,
        &[words([0x7fc0_0000, 0x7f80_0000, 0, 0])],
        |a, _| vec![canonical_nan(a.f64x2().demote_zero(), false)],
    );
    case(
        words([0x7f80_0042, 0x7f80_0000, 0xff80_0000, 0x3fe0_0000]),
        0,
        &[words([0, 0x7fff_ffff, 0x8000_0000, 1])],
        |a, _| vec![a.f32x4().trunc_saturating_i32()],
    );
    case(
        qwords([0xc1e0_0000_0020_0000, 0x41e0_0000_0000_0000]),
        0,
        &[words([0x8000_0000, 0x7fff_ffff, 0, 0])],
        |a, _| vec![a.f64x2().trunc_saturating_i32_zero()],
    );
}

#[test]
fn floating_vector_conversion_boundaries() {
    conversions();
}
