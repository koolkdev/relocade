use crate::{
    support::machine::{expected, Exit, Image, Step},
    test_step::{Event, TestModule},
    Compiler, CompilerConfig, CpuState, ExecutionProfile, MemoryBackend, Segment,
    SegmentDefaultSize::Bits16,
    SegmentProfile, SegmentRegister,
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

    for (profile, cpu, code_page) in [
        (ExecutionProfile::Real16, real, 0x11),
        (SegmentProfile::Segmented16.into(), protected, 0xd0011),
    ] {
        let target = CompilerConfig {
            memory_backend: MemoryBackend::PhysicalBus,
            ..Compiler::new(profile).target()
        };
        let interpreter = TestModule::interpreter_run_with_target(target);
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
        assert_eq!(interpreter.observe(&input, 1), wanted, "{profile:?}");
    }
}
