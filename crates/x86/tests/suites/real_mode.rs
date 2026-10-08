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
#[path = "real_mode/segments.rs"]
mod segments;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
};
use wasm86_x86::{CpuState, ExecutionProfile, Segment, SegmentAttributes, Segments, StoredSegment};

fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.segments = Segments::real_mode();
    image
}

fn cache(segment: Segment, value: u16) -> StoredSegment {
    StoredSegment {
        selector: value,
        base: u32::from(value) * 16,
        limit: 0xffff,
        attributes: SegmentAttributes::from_bits(if segment == Segment::Cs { 7 } else { 5 }),
    }
}

fn retired(image: &Image, bytes: usize) -> CpuState {
    let mut cpu = image.cpu;
    cpu.eip += bytes as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn sequences(engine: Engine, frontend: Frontend) -> ImageSequences {
    ImageSequences::new(engine, frontend, ExecutionProfile::Real16)
}
