//! Floating width conversions round once and preserve precise MXCSR exceptions.

use super::*;

#[path = "width/packed.rs"]
mod packed;

struct ScalarCase {
    source: u64,
    result: u64,
    controls: u32,
    raised: u32,
}

impl ScalarCase {
    fn check(&self, checks: &mut ImageSequences, narrow: bool) {
        let code = encoding(prefix(narrow, false), 0x5a, 0xc0); // XMM0,XMM0
        let mut image = floating_image(&code, self.controls);
        lane(&mut image.cpu.simd.xmm[0], narrow, 0, self.source);
        let fault = self.raised & !(self.controls >> 7) != 0;
        let mut cpu = if fault {
            image.cpu
        } else {
            retire(image.cpu, code.len())
        };
        cpu.simd.mxcsr |= self.raised;
        if !fault {
            lane(&mut cpu.simd.xmm[0], !narrow, 0, self.result);
        }
        checks.check(
            &format!(
                "width narrow={narrow} input={:x} MXCSR={:x}",
                self.source, self.controls
            ),
            &code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: if fault {
                    Exit::SimdFloatingPoint
                } else {
                    Exit::Dispatch(cpu.eip)
                },
            }],
        );
    }
}

fn widen(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, result, raised) in [
        (0, 0, 0),
        (0x8000_0000, 0x8000_0000_0000_0000, 0),
        (0x3fc0_0000, 0x3ff8_0000_0000_0000, 0),
        (0xff7f_ffff, 0xc7ef_ffff_e000_0000, 0),
        (0x7f80_0000, 0x7ff0_0000_0000_0000, 0),
        (0xffc0_0123, 0xfff8_0024_6000_0000, 0),
        (0x7f80_0456, 0x7ff8_008a_c000_0000, 1),
        (1, 0x36a0_0000_0000_0000, 2),
        (0x8000_0001, 0xb6a0_0000_0000_0000, 2),
    ] {
        for controls in [0x1f80, 0x1f00, 0x1e80, 0x9f80, 0x1fc0] {
            let daz = raised == 2 && controls & 0x40 != 0;
            ScalarCase {
                source,
                result: if daz { result & (1_u64 << 63) } else { result },
                controls,
                raised: if daz { 0 } else { raised },
            }
            .check(&mut checks, false);
        }
    }
}

fn rounding(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, results, raised) in [
        (0, [0; 4], [0; 4]),
        (0x8000_0000_0000_0000, [0x8000_0000; 4], [0; 4]),
        (0x3ff8_0000_0000_0000, [0x3fc0_0000; 4], [0; 4]),
        (
            0x3ff0_0000_1000_0000,
            [0x3f80_0000, 0x3f80_0000, 0x3f80_0001, 0x3f80_0000],
            [32; 4],
        ),
        (
            0xbff0_0000_1000_0000,
            [0xbf80_0000, 0xbf80_0001, 0xbf80_0000, 0xbf80_0000],
            [32; 4],
        ),
        (
            0x3ff0_0000_3000_0000,
            [0x3f80_0002, 0x3f80_0001, 0x3f80_0002, 0x3f80_0001],
            [32; 4],
        ),
        (0x47ef_ffff_e000_0000, [0x7f7f_ffff; 4], [0; 4]),
        (
            0x47ef_ffff_f000_0000,
            [0x7f80_0000, 0x7f7f_ffff, 0x7f80_0000, 0x7f7f_ffff],
            [40, 32, 40, 32],
        ),
        (
            0x47f0_0000_0000_0000,
            [0x7f80_0000, 0x7f7f_ffff, 0x7f80_0000, 0x7f7f_ffff],
            [40; 4],
        ),
        (
            0xc7f0_0000_0000_0000,
            [0xff80_0000, 0xff80_0000, 0xff7f_ffff, 0xff7f_ffff],
            [40; 4],
        ),
    ] {
        for rc in 0..4 {
            for mask in [0x1f80, 0x0f80] {
                ScalarCase {
                    source,
                    result: results[rc],
                    controls: mask | (rc as u32) << 13,
                    raised: raised[rc],
                }
                .check(&mut checks, true);
            }
        }
    }
}

fn range_and_special(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, controls, result, raised) in [
        (0x36a0_0000_0000_0000, 0x1f80, 1, 0), // Exact minimum subnormal.
        (0x36a0_0000_0000_0000, 0x1780, 0, 16),
        (0x36a0_0000_0000_0000, 0x9f80, 0, 48), // FTZ also flushes exact tiny results.
        (0xb6a0_0000_0000_0000, 0x9f80, 0x8000_0000, 48),
        (0x36a0_0000_0000_0000, 0x9780, 0, 16), // FTZ is ignored when UM=0.
        (0x3690_0000_0000_0000, 0x1f80, 0, 48), // Half minimum subnormal.
        (0x3690_0000_0000_0000, 0x1780, 0, 16),
        (0x380f_ffff_e000_0000, 0x1f80, 0x0080_0000, 48), // Normal final result, tiny before bounded rounding.
        (0x380f_ffff_e000_0000, 0x1780, 0, 16),
        (0x380f_ffff_e000_0000, 0x9f80, 0, 48),
        (0x380f_ffff_f000_0000, 0x1f80, 0x0080_0000, 32), // Unbounded rounding carries into the normal range.
        (0x380f_ffff_f000_0000, 0x1780, 0x0080_0000, 32),
        (0x380f_ffff_f000_0000, 0x9f80, 0x0080_0000, 32),
        (0x47f0_0000_0000_0000, 0x1b80, 0, 8), // Exact unbounded overflow has no PE when OM=0.
        (0x47f0_0000_0000_0001, 0x1b80, 0, 40),
        (0x7ff0_0000_0000_0000, 0, 0x7f80_0000, 0),
        (0xfff0_0000_0000_0000, 0, 0xff80_0000, 0),
        (0xfff8_0024_6000_0123, 0, 0xffc0_0123, 0),
        (0x7ff0_008a_c000_0123, 0x1f80, 0x7fc0_0456, 1),
        (0x7ff0_008a_c000_0123, 0x1f00, 0, 1),
        (1, 0x1f80, 0, 50),
        (1, 0x1e80, 0, 2), // Unmasked DE suppresses result exceptions.
        (1, 0x1fc0, 0, 0),
        (0x8000_0000_0000_0001, 0x1fc0, 0x8000_0000, 0),
    ] {
        ScalarCase {
            source,
            result,
            controls,
            raised,
        }
        .check(&mut checks, true);
    }
}

fn memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for narrow in [false, true] {
        for packed in [false, true] {
            let code = encoding(prefix(narrow, packed), 0x5a, 0x00);
            let width = (if narrow { 8 } else { 4 }) * if packed { 2 } else { 1 };
            for (address, fault) in [
                (0x5000 - width, None),
                (
                    0x4001,
                    if width == 16 {
                        Some(Exit::GeneralProtection { error: 0 })
                    } else {
                        None
                    },
                ),
                (
                    0x6000,
                    Some(Exit::PageFault {
                        address: 0x6000,
                        error: 0,
                    }),
                ),
            ] {
                let mut image = floating_image(&code, 0x1f80);
                image.cpu.registers.eax = address;
                image.map(4, 0x8000, false);
                let mut input = [0; 16];
                lane(&mut input, narrow, 0, ONE[narrow as usize]);
                image.data(address + 0x4000, &input[..width as usize]);
                let mut cpu = image.cpu;
                if fault.is_none() {
                    cpu = retire(cpu, code.len());
                    if packed {
                        cpu.simd.xmm[0] = [0; 16];
                    }
                    lane(&mut cpu.simd.xmm[0], !narrow, 0, ONE[!narrow as usize]);
                }
                checks.check(
                    "width conversion memory sizes and alignment",
                    &code,
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit: fault.unwrap_or(Exit::Dispatch(cpu.eip)),
                    }],
                );
            }
        }
    }
}

test_frontends!(sse_float_widen, widen);
test_frontends!(sse_float_narrow_rounding, rounding);
test_frontends!(sse_float_narrow_range, range_and_special);
test_frontends!(sse_float_width_memory, memory);
