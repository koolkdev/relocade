//! Arithmetic checks rounding, class priority and precise state publication.

use super::*;

#[path = "arithmetic/add.rs"]
mod add;
#[path = "arithmetic/jit.rs"]
mod jit;

const MIN_NORMAL: [u64; 2] = [0x0080_0000, 0x0010_0000_0000_0000];
const MAX_FINITE: [u64; 2] = [0x7f7f_ffff, 0x7fef_ffff_ffff_ffff];
const INDEFINITE: [u64; 2] = [0xffc0_0000, 0xfff8_0000_0000_0000];
const QUIETED: [u64; 2] = [0x7fc0_0456, 0x7ff8_0000_0000_0456];
const HALF_ULP: [u64; 2] = [0x3380_0000, 0x3ca0_0000_0000_0000];
const NEXT_ONE: [u64; 2] = [0x3f80_0001, 0x3ff0_0000_0000_0001];

struct Case {
    left: [u64; 2],
    right: [u64; 2],
    result: [u64; 2],
    controls: u32,
    raised: u32,
}

impl Case {
    fn check(&self, checks: &mut ImageSequences, opcode: u8) {
        for wide in [false, true] {
            for packed in [false, true] {
                let code = encoding(prefix(wide, packed), opcode, 0xc1);
                let mut image = floating_image(&code, self.controls);
                let count = if packed {
                    if wide {
                        2
                    } else {
                        4
                    }
                } else {
                    1
                };
                for index in 0..count {
                    lane(
                        &mut image.cpu.simd.xmm[0],
                        wide,
                        index,
                        self.left[wide as usize],
                    );
                    lane(
                        &mut image.cpu.simd.xmm[1],
                        wide,
                        index,
                        self.right[wide as usize],
                    );
                }
                // Scalar instructions must not classify or replace upper lanes.
                if !packed {
                    lane(&mut image.cpu.simd.xmm[0], wide, 1, SNAN[wide as usize]);
                    lane(&mut image.cpu.simd.xmm[1], wide, 1, SNAN[wide as usize]);
                }
                let fault = self.raised & !(self.controls >> 7) != 0;
                let mut cpu = if fault {
                    image.cpu
                } else {
                    retire(image.cpu, code.len())
                };
                cpu.simd.mxcsr |= self.raised;
                if !fault {
                    for index in 0..count {
                        lane(
                            &mut cpu.simd.xmm[0],
                            wide,
                            index,
                            self.result[wide as usize],
                        );
                    }
                }
                checks.check(&format!("arithmetic {opcode:x} wide={wide} packed={packed} left={:x} right={:x} MXCSR={:x}",
                    self.left[wide as usize], self.right[wide as usize], self.controls),
                    &code, &image, &[Step { cpu, ram: &[], exit: if fault {
                        Exit::SimdFloatingPoint } else { Exit::Dispatch(cpu.eip) } }]);
            }
        }
    }
}
