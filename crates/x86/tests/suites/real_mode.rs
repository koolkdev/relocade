//! Ordinary real-mode execution shares instruction semantics with protected mode.

#[path = "real_mode/control.rs"]
mod control;
#[path = "real_mode/fetch.rs"]
mod fetch;
#[path = "real_mode/flags.rs"]
mod flags;
#[path = "real_mode/handoff.rs"]
mod handoff;
#[path = "real_mode/interrupt_return.rs"]
mod interrupt_return;
#[path = "real_mode/interrupts.rs"]
mod interrupts;
#[path = "real_mode/memory.rs"]
mod memory;
#[path = "real_mode/ports.rs"]
mod ports;
#[path = "real_mode/segments.rs"]
mod segments;
#[path = "real_mode/string_io.rs"]
mod string_io;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{retire, Exit, Image, Step},
};
use relocade_x86::{CpuState, ExecutionProfile, Segment, SegmentRegister, Segments};

fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.segments = Segments::real_mode();
    image
}

fn cache(segment: Segment, value: u16) -> SegmentRegister {
    SegmentRegister {
        selector: value,
        base: u32::from(value) * 16,
        limit: 0xffff,
        ..SegmentRegister::real_mode(segment, value)
    }
}

fn sequences(frontend: Frontend) -> ImageSequences {
    ImageSequences::new(frontend, ExecutionProfile::Real16)
}
