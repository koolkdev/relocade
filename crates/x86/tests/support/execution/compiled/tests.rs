use super::CompiledExecution;
use crate::support::{
    machine::{Exit, Step},
    x87::{set_control, stack_image, write_value},
};
use relocade_x86::{Compiler, CpuFeature, CpuModel, CpuState, SegmentProfile};

#[test]
fn restart_composes_overlapping_prefix_and_completion_writes() {
    // The store's observed rounding mode differs from the runtime image, so it
    // must restart after both MOVs and then overwrite part of their shared range.
    let code = [
        0xc7, 0x05, 0, 0x40, 0, 0, 0x11, 0x22, 0x33, 0x44, // MOV dword [4000],44332211
        0x66, 0xc7, 0x05, 1, 0x40, 0, 0, 0x55, 0x66, // MOV word [4001],6655
        0xd9, 0x1d, 2, 0x40, 0, 0, // FSTP dword [4002]
    ];
    // A nondefault model requires the fixture to retain the complete target.
    let compiled = Compiler::new(SegmentProfile::Flat32)
        .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false))
        .specialize_on_cpu(&CpuState::default())
        .compile_block(0x1000, &code, 3)
        .unwrap();
    let mut image = stack_image(&code, 0, 0xffff);
    set_control(&mut image.cpu.x87.control, 0x0b7f);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 0, (0x8000_0080_0000_0000, 0x3fff)); // 1 + 2^-24
    image.map(4, 0x8000, true);
    image.data(0x8000, &[0xa6; 8]);

    let mut first = image.cpu;
    first.eip = 0x100a;
    first.instruction_count = 0;
    let mut second = first;
    second.eip = 0x1013;
    second.instruction_count = 1;
    let mut last = second;
    last.eip = 0x1019;
    last.instruction_count = 2;
    last.x87.opcode = 0x011d;
    last.x87.instruction_offset = 0x1013;
    last.x87.instruction_selector = 0x1b;
    last.x87.data_offset = 0x4002;
    last.x87.data_selector = 0x23;
    last.x87.status.top = 1;
    last.x87.status.c1 = 1;
    last.x87.status.precision = 1;
    last.x87.tag_word = 0xffff;

    CompiledExecution::new(&compiled).check_restart(
        "restart and completion compose each instruction's memory patch",
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
        ],
        &Step {
            cpu: last,
            ram: &[(0x8002, &[0x01, 0, 0x80, 0x3f])], // round up to 1 + 2^-23
            exit: Exit::Dispatch(last.eip),
        },
    );
}
