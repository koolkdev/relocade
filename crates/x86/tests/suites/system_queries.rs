//! Queries of the host environment and the active execution mode.

#[path = "system_queries/sgdt.rs"]
mod sgdt;
#[path = "system_queries/sidt.rs"]
mod sidt;
#[path = "system_queries/sldt.rs"]
mod sldt;
#[path = "system_queries/smsw.rs"]
mod smsw;
#[path = "system_queries/str.rs"]
mod str;

use crate::support::{
    blocks::BlockModules,
    machine::{expected, Image, Step},
    step::{DescriptorTableQuery, Engine, Event, SystemSelectorQuery, TestModule},
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

enum Query {
    DescriptorTable(DescriptorTableQuery),
    SystemSelector(SystemSelectorQuery),
}

fn check_query(
    engine: Engine,
    profile: ExecutionProfile,
    code: &[u8],
    image: &Image,
    query: Option<Query>,
    step: Step<'_>,
) {
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, code, 1, profile);
    let mut input = image.input();
    let mut wanted = expected(image, &[step]);
    let event = match query {
        Some(Query::DescriptorTable(reply)) => {
            let event = Event::QueryDescriptorTable { table: reply.table };
            input.descriptor_table_queries.push(reply);
            Some(event)
        }
        Some(Query::SystemSelector(reply)) => {
            let event = Event::QuerySystemSelector {
                register: reply.register,
            };
            input.system_selector_queries.push(reply);
            Some(event)
        }
        None => None,
    };
    if let Some(event) = event {
        wanted.events.insert(0, event);
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
