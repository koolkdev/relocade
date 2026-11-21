use crate::{
    support::machine::{expected, Exit, Image, Step},
    test_step::{CallPatches, Event, TestModule},
    CpuState, ExecutionExit, Segment,
    SegmentDefaultSize::{Bits16, Bits32},
    SegmentRegister,
};

#[test]
fn real_and_protected_segments_reach_the_same_physical_ram_and_device() {
    let mut real = CpuState::real_mode();
    real.segments.cs = SegmentRegister::real_mode(Segment::Cs, 0x1100);
    real.segments.ds = SegmentRegister::real_mode(Segment::Ds, 0x0200);

    // Protected CS:EIP fetches the same instructions above the real-mode address range.
    let mut protected = CpuState::protected_mode(Bits16, Bits16);
    protected.segments.cs.base = 0xd000_1000;
    protected.eip = 0x1_0000;
    protected.segments.ds.base = 0x2000;

    let interpreter =
        TestModule::from_bytes(relocade_interpreter::pc(), crate::abi::RUN.name, None);
    for (cpu, code_page) in [(real, 0x11), (protected, 0xd0011)] {
        let mut image = Image::empty();
        image.cpu = cpu;
        image.data(
            0x4000,
            &[
                0xa1, 0x10, 0x00, // MOV AX,[0010] reads RAM
                0xa3, 0x20, 0x10, // MOV [1020],AX writes the device
                0xa1, 0x30, 0x10, // MOV AX,[1030] reads the device
                0xff, 0xe0, // JMP AX
            ],
        );
        image.data(0x5010, &[0x78, 0x56]);
        image.data(0x8030, &[0x34, 0x12]);
        let mut input = image.input();
        // Only the physical bus has mappings for these guest addresses.
        input.physical_pages = vec![(code_page, 0x4000, true), (2, 0x5000, true)];
        input.mmio_pages = vec![(3, 0x8000)];
        input.observe_mmio = true;

        let mut after = cpu;
        after.registers.eax = 0x1234;
        after.eip = 0x1234;
        after.instruction_count = 4;
        let mut wanted = expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[(0x8020, &[0x78, 0x56])],
                exit: Exit::Dispatch(0x1234),
            }],
        );
        wanted.events.splice(
            0..0,
            [
                Event::MmioWrite {
                    address: 0x3020,
                    value: vec![0x78, 0x56],
                },
                Event::MmioRead {
                    address: 0x3030,
                    bytes: 2,
                },
            ],
        );
        assert_eq!(interpreter.observe(&input, 1), wanted);
    }
}

#[test]
fn same_pc_instance_steps_both_instruction_widths_at_the_same_address() {
    let interpreter =
        TestModule::from_bytes(relocade_interpreter::pc(), crate::abi::STEP.name, None);
    let mut image = Image::new(&[0xb8, 0x34, 0x12, 0x78, 0x56]);
    image.cpu = CpuState::real_mode();
    image.cpu.eip = 0x1000;
    image.cpu.registers.eax = 0xabcd_0000;
    let mut protected = CpuState::protected_mode(Bits32, Bits16);
    protected.eip = image.cpu.eip;
    protected.registers.eax = image.cpu.registers.eax;

    let mut input = image.input();
    input.patches_before_calls = vec![
        CallPatches::default(),
        CallPatches::cpu(vec![(0, protected.to_bytes().to_vec())]),
    ];
    let mut first = image.cpu;
    first.eip += 3;
    first.registers.eax = 0xabcd_1234;
    first.instruction_count += 1;
    let mut second = protected;
    second.eip += 5;
    second.registers.eax = 0x5678_1234;
    second.instruction_count += 1;
    let steps = [first, second].map(|cpu| Step {
        cpu,
        ram: &[],
        exit: Exit::Other(ExecutionExit::StepComplete),
    });
    assert_eq!(interpreter.observe(&input, 2), expected(&image, &steps));
}
