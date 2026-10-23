//! Shared observations for instructions that query a selector without loading it.

use super::selector_cases::code_defaults;
use crate::support::machine::Image;
use crate::{CpuState, ExecutionProfile};

pub(super) fn image(code: &[u8], profile: ExecutionProfile) -> Image {
    let mut image = Image::new(code);
    code_defaults(&mut image, profile);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.cf = 1;
    image.cpu.flags.bytes.pf = 0;
    image.cpu.flags.bytes.af = 1;
    image.cpu.flags.bytes.zf = 0;
    image.cpu.flags.bytes.sf = 0;
    image.cpu.flags.bytes.of = 1;
    image
}

pub(super) fn completed(image: &Image, len: usize, zf: bool) -> CpuState {
    let mut cpu = image.cpu;
    cpu.flags.bytes.zf = u8::from(zf);
    cpu.eip += len as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}
