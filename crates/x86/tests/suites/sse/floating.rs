//! Floating-point fixtures retain opaque upper lanes and unrelated machine state.

#[path = "floating/flags.rs"]
mod flags;
#[path = "floating/masks.rs"]
mod masks;
#[path = "floating/minmax.rs"]
mod minmax;

use super::*;

const ZERO: [u64; 2] = [0, 0];
const NEG_ZERO: [u64; 2] = [0x8000_0000, 0x8000_0000_0000_0000];
const ONE: [u64; 2] = [0x3f80_0000, 0x3ff0_0000_0000_0000];
const NEG_ONE: [u64; 2] = [0xbf80_0000, 0xbff0_0000_0000_0000];
const TWO: [u64; 2] = [0x4000_0000, 0x4000_0000_0000_0000];
const NEG_TWO: [u64; 2] = [0xc000_0000, 0xc000_0000_0000_0000];
const INFINITY: [u64; 2] = [0x7f80_0000, 0x7ff0_0000_0000_0000];
const NEG_INFINITY: [u64; 2] = [0xff80_0000, 0xfff0_0000_0000_0000];
const QNAN: [u64; 2] = [0x7fc0_0123, 0x7ff8_0000_0000_0123];
const SNAN: [u64; 2] = [0x7f80_0456, 0x7ff0_0000_0000_0456];
const DENORMAL: [u64; 2] = [1, 1];
const NEG_DENORMAL: [u64; 2] = [0x8000_0001, 0x8000_0000_0000_0001];

fn prefix(wide: bool, packed: bool) -> &'static [u8] {
    match (wide, packed) {
        (false, false) => &[0xf3],
        (true, false) => &[0xf2],
        (false, true) => &[],
        (true, true) => &[0x66],
    }
}

fn lane(vector: &mut [u8; 16], wide: bool, index: usize, bits: u64) {
    let width = if wide { 8 } else { 4 };
    vector[index * width..(index + 1) * width].copy_from_slice(&bits.to_le_bytes()[..width]);
}

fn floating_image(code: &[u8], mxcsr: u32) -> Image {
    let mut image = image(code);
    image.cpu.simd.mxcsr = mxcsr;
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.cf = 1;
    image.cpu.flags.bytes.pf = 1;
    image.cpu.flags.bytes.af = 1;
    image.cpu.flags.bytes.zf = 1;
    image.cpu.flags.bytes.sf = 1;
    image.cpu.flags.bytes.of = 1;
    image
}

fn comparison_flags(cpu: &mut CpuState, flags: u8) {
    cpu.flags.status_source.kind = 0;
    cpu.flags.bytes.cf = flags & 1;
    cpu.flags.bytes.pf = (flags >> 2) & 1;
    cpu.flags.bytes.zf = (flags >> 6) & 1;
    cpu.flags.bytes.af = 0;
    cpu.flags.bytes.sf = 0;
    cpu.flags.bytes.of = 0;
}
