//! Queries of the host environment and the active execution mode.

#[path = "system_queries/sgdt.rs"]
mod sgdt;
#[path = "system_queries/smsw.rs"]
mod smsw;

use crate::support::{
    blocks::BlockModules,
    machine::{expected, Image, Step},
    step::{DescriptorTableQuery, Engine, Event, TestModule},
};
use crate::{CpuState, ExecutionProfile, SegmentAttributes, SegmentProfile, Segments};

fn image(code: &[u8], profile: ExecutionProfile) -> Image {
    let mut image = Image::new(code);
    match profile {
        ExecutionProfile::Real16 => image.cpu.segments = Segments::real_mode(),
        ExecutionProfile::Protected(SegmentProfile::Segmented16) => {
            image.cpu.segments.cs.attributes = SegmentAttributes::from_bits(7);
        }
        _ => {}
    }
    image
}

fn completed(image: &Image, length: usize) -> CpuState {
    let mut cpu = image.cpu;
    cpu.eip += length as u32;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu
}

fn check_table(
    engine: Engine,
    profile: ExecutionProfile,
    code: &[u8],
    image: &Image,
    queries: &[DescriptorTableQuery],
    step: Step<'_>,
) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, 1, profile);
    let mut input = image.input();
    input.descriptor_table_queries = queries.to_vec();
    let mut wanted = expected(image, &[step]);
    for (index, query) in queries.iter().enumerate() {
        wanted
            .events
            .insert(index, Event::QueryDescriptorTable { table: query.table });
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
