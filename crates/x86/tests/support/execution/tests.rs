use super::{test_frontends, Frontend, ImageSequences};
use crate::support::machine::{Exit, Image, Step};
use relocade_x86::{ExecutionProfile, SegmentProfile};

fn overlapping_prefix_writes(frontend: Frontend) {
    for split in [false, true] {
        check_overlapping_prefix_writes(frontend, split);
    }
}

fn check_overlapping_prefix_writes(frontend: Frontend, split: bool) {
    // MOV dword [4000],44332211; MOV word [4001],6655; NOP.
    let mut code = vec![
        0xc7, 0x05, 0, 0x40, 0, 0, 0x11, 0x22, 0x33, 0x44, 0x66, 0xc7, 0x05, 1, 0x40, 0, 0, 0x55,
        0x66, 0x90,
    ];
    if split {
        // A real block boundary must carry the first write into the next block.
        code.splice(10..10, [0xeb, 0x00]); // JMP +0
        let before_nop = code.len() - 1;
        code.splice(before_nop..before_nop, [0xa1, 0, 0x40, 0, 0]); // MOV EAX,[4000]
    }
    let mut image = Image::new(&code);
    image.map(4, 0x8000, true);
    image.data(0x8000, &[0x88, 0x99, 0xaa, 0xbb]);
    let mut first = image.cpu;
    first.eip = 0x100a;
    first.instruction_count = 0;
    let mut second = first;
    second.eip = if split { 0x1015 } else { 0x1013 };
    second.instruction_count = if split { 2 } else { 1 };
    let mut steps = vec![Step {
        cpu: first,
        ram: &[(0x8000, &[0x11, 0x22, 0x33, 0x44])],
        exit: Exit::Dispatch(first.eip),
    }];
    if split {
        let mut branch = first;
        branch.eip = 0x100c;
        branch.instruction_count = 1;
        steps.push(Step {
            cpu: branch,
            ram: &[],
            exit: Exit::Dispatch(branch.eip),
        });
    }
    steps.push(Step {
        cpu: second,
        ram: &[(0x8001, &[0x55, 0x66])],
        exit: Exit::Dispatch(second.eip),
    });
    let mut last = second;
    if split {
        // Independent readback detects lost bytes when carrying RAM across blocks.
        last.registers.eax = 0x4466_5511;
        last.eip = 0x101a;
        last.instruction_count = 3;
        steps.push(Step {
            cpu: last,
            ram: &[],
            exit: Exit::Dispatch(last.eip),
        });
    }
    last.eip += 1;
    last.instruction_count += 1;
    steps.push(Step {
        cpu: last,
        ram: &[],
        exit: Exit::Dispatch(last.eip),
    });
    ImageSequences::new(
        frontend,
        ExecutionProfile::Protected(SegmentProfile::Flat32),
    )
    .check(
        "a final empty patch retains and composes both earlier overlapping writes",
        &code,
        &image,
        &steps,
    );
}

test_frontends!(
    prefix_memory_changes_survive_the_final_step,
    overlapping_prefix_writes
);
