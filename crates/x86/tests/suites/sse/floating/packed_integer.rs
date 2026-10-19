//! Packed integer conversions aggregate exceptions and publish the entire result.

use super::*;

fn conversion_code(wide: bool, truncate: bool, modrm: u8) -> Vec<u8> {
    let prefix = match (wide, truncate) {
        (false, false) | (true, true) => 0x66,
        (false, true) => 0xf3,
        (true, false) => 0xf2,
    };
    encoding(&[prefix], if wide { 0xe6 } else { 0x5b }, modrm)
}

fn lanes(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for truncate in [false, true] {
            let code = conversion_code(wide, truncate, 0xc0); // XMM0,XMM0
            for rc in 0..4 {
                let mut image = floating_image(&code, 0x1f80 | rc << 13);
                for (index, value) in [1.5_f64, -2.5, 0.0, -2147483648.0]
                    .into_iter()
                    .enumerate()
                    .take(if wide { 2 } else { 4 })
                {
                    let bits = if wide {
                        value.to_bits()
                    } else {
                        (value as f32).to_bits() as u64
                    };
                    lane(&mut image.cpu.simd.xmm[0], wide, index, bits);
                }
                let mode = if truncate { 3 } else { rc as usize };
                let integers = [[2_i32, -2], [1, -3], [2, -2], [1, -2]][mode];
                let mut cpu = retire(image.cpu, code.len());
                cpu.simd.mxcsr |= 32;
                cpu.simd.xmm[0] = [0; 16];
                for (index, value) in [integers[0], integers[1], 0, i32::MIN]
                    .into_iter()
                    .enumerate()
                    .take(if wide { 2 } else { 4 })
                {
                    lane(&mut cpu.simd.xmm[0], false, index, value as u32 as u64);
                }
                checks.check(
                    "packed conversion rounds every lane, including destination aliases",
                    &code,
                    &image,
                    &[dispatch(cpu)],
                );
            }
        }
    }
}

fn exceptions(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        let code = conversion_code(wide, false, 0xc1);
        for (mxcsr, raised, fault) in [
            (0x1f80, 33, false),
            (0x1f00, 1, true), // Invalid blocks the sibling's precision flag.
            (0x0f80, 33, true),
            (0x1f20, 33, true), // Old sticky PE survives the invalid fault.
        ] {
            let mut image = floating_image(&code, mxcsr);
            image.cpu.simd.xmm[1] = [0; 16];
            lane(&mut image.cpu.simd.xmm[1], wide, 0, QNAN[wide as usize]);
            lane(
                &mut image.cpu.simd.xmm[1],
                wide,
                1,
                if wide {
                    1.5_f64.to_bits()
                } else {
                    1.5_f32.to_bits() as u64
                },
            );
            let mut cpu = if fault {
                image.cpu
            } else {
                retire(image.cpu, code.len())
            };
            cpu.simd.mxcsr |= raised;
            if !fault {
                cpu.simd.xmm[0] = [0; 16];
                lane(&mut cpu.simd.xmm[0], false, 0, 0x8000_0000);
                lane(&mut cpu.simd.xmm[0], false, 1, 2);
            }
            checks.check(
                "packed conversion collects conditions before any destination update",
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
}

fn memory(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        let code = conversion_code(wide, true, 0x00);
        for (address, fault) in [
            (0x4ff0, None),
            (0x4001, Some(Exit::GeneralProtection { error: 0 })),
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
            lane(&mut input, wide, 0, ONE[wide as usize]);
            image.data(0x8ff0, &input);
            let mut cpu = image.cpu;
            if fault.is_none() {
                cpu = retire(cpu, code.len());
                cpu.simd.xmm[0] = [0; 16];
                lane(&mut cpu.simd.xmm[0], false, 0, 1);
            }
            checks.check(
                "packed conversion requires an aligned m128 operand",
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

test_frontends!(sse_packed_integer_lanes, lanes);
test_frontends!(sse_packed_integer_exceptions, exceptions);
test_frontends!(sse_packed_integer_memory, memory);

fn boundaries(frontend: Frontend) {
    let mut checks = ImageSequences::new(frontend, SegmentProfile::Flat32);
    for truncate in [false, true] {
        let code = conversion_code(true, truncate, 0xc1);
        for rc in 0..4 {
            let mut image = floating_image(&code, 0x1f80 | rc << 13);
            lane(&mut image.cpu.simd.xmm[1], true, 0, 0xc1e0_0000_0010_0000); // -2^31 - 0.5
            lane(&mut image.cpu.simd.xmm[1], true, 1, 0x41df_ffff_ffe0_0000); // 2^31 - 0.5
            let mode = if truncate { 3 } else { rc };
            let mut cpu = retire(image.cpu, code.len());
            cpu.simd.mxcsr |= if mode == 3 { 32 } else { 33 };
            cpu.simd.xmm[0] = [0; 16];
            lane(&mut cpu.simd.xmm[0], false, 0, 0x8000_0000);
            lane(
                &mut cpu.simd.xmm[0],
                false,
                1,
                if mode == 1 || mode == 3 {
                    0x7fff_ffff
                } else {
                    0x8000_0000
                },
            );
            checks.check(
                "packed bounds are checked after rounding, before saturating conversion",
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
    for wide in [false, true] {
        let code = conversion_code(wide, false, 0xc1);
        for daz in [false, true] {
            for rc in 0..4 {
                let mut image = floating_image(&code, 0x1f80 | rc << 13 | u32::from(daz) << 6);
                image.cpu.simd.xmm[1] = [0; 16];
                lane(&mut image.cpu.simd.xmm[1], wide, 0, DENORMAL[wide as usize]);
                lane(
                    &mut image.cpu.simd.xmm[1],
                    wide,
                    1,
                    NEG_DENORMAL[wide as usize],
                );
                let mut cpu = retire(image.cpu, code.len());
                cpu.simd.mxcsr |= if daz { 0 } else { 32 };
                cpu.simd.xmm[0] = [0; 16];
                lane(&mut cpu.simd.xmm[0], false, 0, u64::from(!daz && rc == 2));
                lane(
                    &mut cpu.simd.xmm[0],
                    false,
                    1,
                    if !daz && rc == 1 { 0xffff_ffff } else { 0 },
                );
                checks.check(
                    "packed integer conversion normalizes DAZ without denormal status",
                    &code,
                    &image,
                    &[dispatch(cpu)],
                );
            }
        }
    }
}

test_frontends!(sse_packed_integer_boundaries, boundaries);
