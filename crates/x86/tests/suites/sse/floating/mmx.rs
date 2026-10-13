//! Conversion bridges retain physical MMX aliases and Intel's x87 fault ordering.

use super::*;
use crate::support::x87::{stack_image, write_register_bits};

fn mmx_image(code: &[u8], controls: u32) -> Image {
    let mut image = floating_image(code, controls);
    image.cpu.x87 = stack_image(code, 5, 0xffff).cpu.x87;
    image
}

fn from_integer(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for memory in [false, true] {
            let code = encoding(prefix(wide, true), 0x2a, if memory { 0x00 } else { 0xc1 });
            for rc in 0..4 {
                for mask in [0x1f80, 0x0f80] {
                    let mut image = mmx_image(&code, mask | rc << 13);
                    let source = 0xffff_ffff_0100_0001_u64; // [16777217, -1]
                    write_register_bits(&mut image.cpu, 1, (source, 0x1234));
                    image.cpu.registers.eax = 0x4ff8;
                    image.map(4, 0x8000, false);
                    image.data(0x8ff8, &source.to_le_bytes());
                    let fault = !wide && mask == 0x0f80;
                    let mut cpu = if fault {
                        image.cpu
                    } else {
                        retire(image.cpu, code.len())
                    };
                    if !wide {
                        cpu.simd.mxcsr |= 32;
                    }
                    if !fault {
                        if !wide || !memory {
                            cpu.x87.status.top = 0;
                            cpu.x87.tag_word = 0;
                        }
                        lane(
                            &mut cpu.simd.xmm[0],
                            wide,
                            0,
                            if wide {
                                16777217.0_f64.to_bits()
                            } else if rc == 2 {
                                0x4b80_0001
                            } else {
                                0x4b80_0000
                            },
                        );
                        lane(&mut cpu.simd.xmm[0], wide, 1, NEG_ONE[wide as usize]);
                    }
                    checks.check(
                        "MMX integer source uses physical slot and preserves x87 pointers",
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

fn to_integer(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for wide in [false, true] {
        for truncate in [false, true] {
            let code = encoding(prefix(wide, true), if truncate { 0x2c } else { 0x2d }, 0xc1);
            for (first, second, integers, status) in [
                (ONE, NEG_ONE, [[1, -1]; 4], 0),
                (
                    [0x3fc0_0000, 0x3ff8_0000_0000_0000],
                    [0xbfc0_0000, 0xbff8_0000_0000_0000],
                    [[2, -2], [1, -2], [2, -1], [1, -1]],
                    32,
                ),
                (QNAN, ONE, [[i32::MIN, 1]; 4], 1),
                (
                    DENORMAL,
                    NEG_DENORMAL,
                    [[0, 0], [0, -1], [1, 0], [0, 0]],
                    32,
                ),
            ] {
                for rc in 0..4 {
                    for masks in [0x1f80, 0x1f00, 0x0f80, 0x1ec0] {
                        let mut image = mmx_image(&code, masks | (rc as u32) << 13);
                        lane(&mut image.cpu.simd.xmm[1], wide, 0, first[wide as usize]);
                        lane(&mut image.cpu.simd.xmm[1], wide, 1, second[wide as usize]);
                        let daz = first == DENORMAL && masks & 0x40 != 0;
                        let raised = if daz { 0 } else { status };
                        let fault = raised & !(masks >> 7) != 0;
                        let mut cpu = if fault {
                            image.cpu
                        } else {
                            retire(image.cpu, code.len())
                        };
                        cpu.simd.mxcsr |= raised;
                        if !fault {
                            let values = if daz {
                                [0, 0]
                            } else {
                                integers[if truncate { 3 } else { rc }]
                            };
                            let result =
                                (values[0] as u32 as u64) | ((values[1] as u32 as u64) << 32);
                            write_register_bits(&mut cpu, 0, (result, 0xffff));
                            cpu.x87.status.top = 0;
                            cpu.x87.tag_word = 0;
                        }
                        checks.check("float to MMX commits both dwords and environment after exception checks", &code, &image,
                            &[Step { cpu, ram: &[], exit: if fault { Exit::SimdFloatingPoint } else { Exit::Dispatch(cpu.eip) } }]);
                    }
                }
            }
        }
    }
}

fn memory_and_pending(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (wide, opcode) in [
        (false, 0x2a),
        (true, 0x2a),
        (false, 0x2d),
        (true, 0x2d),
        (false, 0x2c),
        (true, 0x2c),
    ] {
        let code = encoding(prefix(wide, true), opcode, 0x00);
        let width = if wide && opcode != 0x2a { 16 } else { 8 };
        let uses_mmx = !(wide && opcode == 0x2a);
        for pending in [false, true] {
            for address in [0x5000 - width, 0x4001, 0x6000] {
                let mut image = mmx_image(&code, 0x1f80);
                image.cpu.registers.eax = address;
                image.cpu.x87.status.error_summary = u8::from(pending);
                image.map(4, 0x8000, false);
                let mut input = [0; 16];
                lane(
                    &mut input,
                    wide && opcode != 0x2a,
                    0,
                    if opcode == 0x2a {
                        1
                    } else {
                        ONE[wide as usize]
                    },
                );
                image.data(address + 0x4000, &input[..width as usize]);
                let fault = if uses_mmx && pending {
                    Some(Exit::FloatingPoint)
                } else if width == 16 && address & 15 != 0 {
                    Some(Exit::GeneralProtection { error: 0 })
                } else if address == 0x6000 {
                    Some(Exit::PageFault { address, error: 0 })
                } else {
                    None
                };
                let mut cpu = image.cpu;
                if fault.is_none() {
                    cpu = retire(cpu, code.len());
                    if uses_mmx {
                        cpu.x87.status.top = 0;
                        cpu.x87.tag_word = 0;
                    }
                    if opcode == 0x2a {
                        lane(&mut cpu.simd.xmm[0], wide, 0, ONE[wide as usize]);
                        lane(&mut cpu.simd.xmm[0], wide, 1, 0);
                    } else {
                        write_register_bits(&mut cpu, 0, (1, 0xffff));
                    }
                }
                checks.check(
                    "Intel MMX admission precedes memory except CVTPI2PD m64",
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
    // The exempt memory form must not exempt CVTPI2PD's MMX register form.
    let code = [0x66, 0x0f, 0x2a, 0xc1];
    let mut image = mmx_image(&code, 0x1f80);
    image.cpu.x87.status.error_summary = 1;
    checks.check(
        "CVTPI2PD register source checks pending x87 exception",
        &code,
        &image,
        &[Step {
            cpu: image.cpu,
            ram: &[],
            exit: Exit::FloatingPoint,
        }],
    );
}

test_frontends!(sse_mmx_from_integer, from_integer);
test_frontends!(sse_mmx_to_integer, to_integer);
test_frontends!(sse_mmx_memory_and_pending, memory_and_pending);
