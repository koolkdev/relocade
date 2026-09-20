//! Shared observations for instructions that query a selector without loading it.

use super::selector_cases::code_defaults;
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Image, Step},
    step::{Engine, Event, SegmentQuery, TestModule},
};
use crate::{CpuState, SegmentProfile};

pub(super) fn image(code: &[u8], profile: SegmentProfile) -> Image {
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

pub(super) fn check_one(
    engine: Engine,
    profile: SegmentProfile,
    code: &[u8],
    image: &Image,
    queries: &[SegmentQuery],
    step: Step<'_>,
) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, 1, profile);
    let mut input = image.input();
    input.segment_queries = queries.to_vec();
    let mut wanted = expected(image, &[step]);
    for (index, query) in queries.iter().enumerate() {
        wanted.events.insert(
            index,
            Event::QuerySegmentDescriptor {
                selector: i32::from(query.selector),
            },
        );
    }
    for module in [block, TestModule::interpreter_with_profile(profile)] {
        assert_eq!(
            engine.observe(module, &input, 1),
            wanted,
            "{} {profile:?} {code:02x?}",
            module.entry
        );
    }
}
