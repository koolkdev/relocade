use super::{test_frontends, Frontend, ImageSequences};
use crate::support::{
    machine::{Exit, Image, Step},
    step::Engine,
};
use wasm86_x86::SegmentProfile;

fn overlapping_prefix_writes(engine: Engine, frontend: Frontend) {
    // MOV dword [4000],44332211; MOV word [4001],6655; NOP.
    let code = [
        0xc7, 0x05, 0, 0x40, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x66, 0xc7, 0x05, 1, 0x40, 0, 0, 0x55,
        0x66, 0x90,
    ];
    let mut image = Image::new(&code);
    image.map(4, 0x8000, true);
    image.data(0x8000, &[0x88, 0x99, 0xaa, 0xbb]);
    let mut first = image.cpu;
    first.eip = 0x100a;
    first.instruction_count = 0;
    let mut second = first;
    second.eip = 0x1013;
    second.instruction_count = 1;
    let mut last = second;
    last.eip = 0x1014;
    last.instruction_count = 2;
    ImageSequences::new(engine, frontend, SegmentProfile::Flat32).check(
        "a final empty patch retains and composes both earlier overlapping writes",
        &code,
        &image,
        &[
            Step {
                cpu: first,
                ram: &[(0x8000, &[0x11, 0x22, 0x33, 0x44])],
                exit: Exit::Dispatch(first.eip),
            },
            Step {
                cpu: second,
                ram: &[(0x8001, &[0x55, 0x66])],
                exit: Exit::Dispatch(second.eip),
            },
            Step {
                cpu: last,
                ram: &[],
                exit: Exit::Dispatch(last.eip),
            },
        ],
    );
}

test_frontends!(
    prefix_memory_changes_survive_the_final_step,
    overlapping_prefix_writes
);
