//! Checks for the complete architectural span of aligned m128 operands.

use super::*;

/// `code` must address m128 at [0x4000] with 16-bit addressing.
pub(super) fn check_m128_span(checks: &mut ImageSequences, code: &[u8]) {
    let mut image = image(code);
    image.cpu.segments.cs.big = 0;
    image.cpu.segments.ds.limit = 0x4007;
    image.cpu.simd.xmm[6] = [0; 16];
    image.map(4, 0x8000, false);
    image.data(0x8000, &[0; 16]);
    checks.check(
        "unused source bits still belong to the m128 operand span",
        code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        }],
    );
}
