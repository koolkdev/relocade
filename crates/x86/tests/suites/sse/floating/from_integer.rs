//! Integer sources round to binary32; binary64 conversion is always exact.

use super::*;

fn conversion_code(wide: bool, packed: bool, modrm: u8) -> Vec<u8> {
    if packed {
        encoding(
            if wide { &[0xf3] } else { &[] },
            if wide { 0xe6 } else { 0x5b },
            modrm,
        )
    } else {
        encoding(prefix(wide, false), 0x2a, modrm)
    }
}

fn scalar(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        let code = conversion_code(wide, false, 0xc0); // XMM0,EAX
        for (integer, singles, inexact) in [
            (0_i32, [0_u32; 4], false),
            (1, [0x3f80_0000; 4], false),
            (-1, [0xbf80_0000; 4], false),
            (16777216, [0x4b80_0000; 4], false),
            (
                16777217,
                [0x4b80_0000, 0x4b80_0000, 0x4b80_0001, 0x4b80_0000],
                true,
            ),
            (
                16777219,
                [0x4b80_0002, 0x4b80_0001, 0x4b80_0002, 0x4b80_0001],
                true,
            ),
            (
                -16777217,
                [0xcb80_0000, 0xcb80_0001, 0xcb80_0000, 0xcb80_0000],
                true,
            ),
            (
                i32::MAX,
                [0x4f00_0000, 0x4eff_ffff, 0x4f00_0000, 0x4eff_ffff],
                true,
            ),
            (i32::MIN, [0xcf00_0000; 4], false),
            (
                -2147483647,
                [0xcf00_0000, 0xcf00_0000, 0xceff_ffff, 0xceff_ffff],
                true,
            ),
        ] {
            for rc in 0..4 {
                for mask in [0x1f80, 0x0f80] {
                    let mut image = floating_image(&code, mask | (rc as u32) << 13);
                    image.cpu.registers.eax = integer as u32;
                    let precision = !wide && inexact;
                    let fault = precision && mask == 0x0f80;
                    let mut cpu = if fault {
                        image.cpu
                    } else {
                        retire(image.cpu, code.len())
                    };
                    cpu.simd.mxcsr |= if precision { 32 } else { 0 };
                    if !fault {
                        lane(
                            &mut cpu.simd.xmm[0],
                            wide,
                            0,
                            if wide {
                                f64::from(integer).to_bits()
                            } else {
                                singles[rc] as u64
                            },
                        );
                    }
                    let name =
                        format!("integer {integer} to float wide={wide} RC={rc} mask={mask:x}");
                    checks.check(
                        &name,
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
    }
}

fn packed(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        let code = conversion_code(wide, true, 0xc0); // XMM0,XMM0
        for rc in 0..4 {
            let mut image = floating_image(&code, 0x1f80 | rc << 13);
            for (index, value) in [16777217_i32, -1, i32::MAX, i32::MIN]
                .into_iter()
                .enumerate()
            {
                lane(
                    &mut image.cpu.simd.xmm[0],
                    false,
                    index,
                    value as u32 as u64,
                );
            }
            let mut cpu = retire(image.cpu, code.len());
            if wide {
                lane(&mut cpu.simd.xmm[0], true, 0, 16777217.0_f64.to_bits());
                lane(&mut cpu.simd.xmm[0], true, 1, (-1.0_f64).to_bits());
            } else {
                let first = if rc == 2 { 0x4b80_0001 } else { 0x4b80_0000 };
                let third = if rc == 1 || rc == 3 {
                    0x4eff_ffff
                } else {
                    0x4f00_0000
                };
                for (index, value) in [first, 0xbf80_0000, third, 0xcf00_0000]
                    .into_iter()
                    .enumerate()
                {
                    lane(&mut cpu.simd.xmm[0], false, index, value);
                }
                cpu.simd.mxcsr |= 32;
            }
            checks.check(
                "packed integer conversion reads only consumed dwords and supports aliases",
                &code,
                &image,
                &[dispatch(cpu)],
            );
        }
    }
}

fn memory(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for packed in [false, true] {
            let code = conversion_code(wide, packed, 0x00);
            let width = if packed {
                if wide {
                    8
                } else {
                    16
                }
            } else {
                4
            };
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
                input[0] = 1;
                image.data(address + 0x4000, &input[..width as usize]);
                let mut cpu = image.cpu;
                if fault.is_none() {
                    cpu = retire(cpu, code.len());
                    if packed {
                        cpu.simd.xmm[0] = [0; 16];
                    }
                    lane(&mut cpu.simd.xmm[0], wide, 0, ONE[wide as usize]);
                }
                checks.check(
                    "integer conversion source width and alignment",
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

test_frontends!(sse_float_from_integer_scalar, scalar);
test_frontends!(sse_float_from_integer_packed, packed);
test_frontends!(sse_float_from_integer_memory, memory);
