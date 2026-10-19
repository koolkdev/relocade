//! Whole-register byte shifts cross lane boundaries and never mask large counts.

use super::*;

fn shifts(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    let source: [u8; 16] = std::array::from_fn(|byte| 0x80 + byte as u8);
    for left in [false, true] {
        for count in [0, 1, 7, 8, 9, 15, 16, 17, 31, 32, 127, 128, 240, 241, 255] {
            let code = [0x66, 0x0f, 0x73, if left { 0xfe } else { 0xde }, count];
            let mut image = image(&code);
            image.cpu.simd.xmm[6] = source;
            image.cpu.x87.status.error_summary = 1;
            let mut cpu = retire(image.cpu, code.len());
            let mut result = [0; 16];
            let count = usize::from(count).min(16);
            if left {
                result[count..].copy_from_slice(&source[..16 - count]);
            } else {
                result[..16 - count].copy_from_slice(&source[count..]);
            }
            cpu.simd.xmm[6] = result;
            checks.check(
                "byte shift zero-fills across the qword boundary and preserves flags",
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
}

#[test]
fn sse_byte_shifts_reject_memory_and_mmx_forms() {
    for code in [
        &[0x66, 0x0f, 0x73, 0x3c][..],
        &[0x66, 0x0f, 0x73, 0x1c],
        &[0x0f, 0x73, 0xf8],
        &[0x0f, 0x73, 0xd8],
    ] {
        crate::support::encoding::check_unsupported_at_page_end(code, 0x0f);
    }
}

test_frontends!(sse_whole_register_byte_shifts, shifts);
