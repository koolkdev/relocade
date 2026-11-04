//! HLT retires before waiting; only real mode can halt in the supported profiles.

use crate::support::{
    blocks::BlockModules,
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit, Image, Step},
    step::TestModule,
};
use relocade_x86::{ExecutionExit, ExecutionProfile, SegmentProfile, Segments};

fn forms(frontend: Frontend) {
    for profile in [
        ExecutionProfile::Real16,
        SegmentProfile::Flat32.into(),
        SegmentProfile::Segmented32.into(),
        SegmentProfile::Segmented16.into(),
    ] {
        let mut cases = ImageSequences::new(frontend, profile);
        for code in [&[0xf4][..], &[0x66, 0x67, 0x64, 0xf4]] {
            for enabled in [false, true] {
                let mut image = Image::new(code);
                if profile == ExecutionProfile::Real16 {
                    image.cpu.segments = Segments::real_mode();
                } else if profile == SegmentProfile::Segmented16.into() {
                    image.cpu.segments.cs.big = 0;
                }
                image.cpu.flags.bytes.if_ = u8::from(enabled);
                image.cpu.flags.bytes.iopl = 3; // IOPL does not permit CPL3 HLT.
                let mut cpu = image.cpu;
                let exit = if profile == ExecutionProfile::Real16 {
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
                    Exit::Other(ExecutionExit::Halted)
                } else {
                    Exit::GeneralProtection { error: 0 }
                };
                cases.check(
                    "HLT preserves the entire flags backing and requires CPL0",
                    code,
                    &image,
                    &[Step {
                        cpu,
                        ram: &[],
                        exit,
                    }],
                );
            }
        }
    }
}
test_frontends!(retirement_and_privilege, forms);

#[test]
fn halt_does_not_fetch_its_successor() {
    // MOV AX,1234; HLT at the end of the mapped page. The next page is absent.
    let code = [0xb8, 0x34, 0x12, 0xf4];
    let mut image = Image::empty();
    image.cpu.segments = Segments::real_mode();
    image.cpu.eip = 0x1ffc;
    image.map(1, 0x3000, false);
    image.data(0x3ffc, &code);
    let mut cpu = image.cpu;
    cpu.eip = 0x2000;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(2);
    cpu.registers.eax = 0x1111_1234;
    let wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Other(ExecutionExit::Halted),
        }],
    );
    let mut blocks = BlockModules::default();
    let block = blocks.get(&image.cpu, &code, 16, ExecutionProfile::Real16);
    let run = TestModule::interpreter_run_with_profile(ExecutionProfile::Real16);
    for module in [block, run] {
        assert_eq!(module.observe(&image.input(), 1), wanted);
    }
}
