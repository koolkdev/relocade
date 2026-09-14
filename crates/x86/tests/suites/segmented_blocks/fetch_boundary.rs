use super::code;
use crate::support::{
    blocks::BlockModules,
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{SegmentDefaultSize, SegmentProfile, StoredSegment};

fn valid_prefix_then_fetch_fault(engine: Engine) {
    let mut blocks = BlockModules::default();
    for (size, profile, modrm) in [
        (
            SegmentDefaultSize::Bits16,
            SegmentProfile::Segmented16,
            0x46,
        ),
        (
            SegmentDefaultSize::Bits32,
            SegmentProfile::Segmented32,
            0x45,
        ),
    ] {
        // ADD AL,1 fits. The following MOV [BP/EBP+0],AX/EAX needs a byte
        // outside CS, so the snapshot producer includes only the ADD.
        let bytes = [0x04, 1, 0x89, modrm, 0, 0x90];
        let mut image = Image::empty();
        image.cpu.registers.eax = 0xaaaa_007f;
        image.cpu.registers.ebp = 0x4000;
        image.cpu.segments.cs = code(0x6003, 0x1003, size);
        image.cpu.segments.ss = StoredSegment::unusable(0x23);
        image.map(7, 0x3000, false);
        image.data(0x3003, &bytes);

        let block = blocks.get(&image.cpu, &bytes, 1, profile);
        let mut cpu = image.cpu;
        cpu.registers.eax = 0xaaaa_0080;
        cpu.flags.status_source.kind = 2;
        cpu.flags.status_source.left = 0x7f;
        cpu.flags.status_source.right = 1;
        cpu.eip = 0x1002;
        cpu.instruction_count = 0;
        assert_eq!(
            engine.observe(block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(0x1002),
                }],
            ),
            "{profile:?} valid block prefix",
        );

        // Enter the interpreter with the verified prefix result. Fetch must
        // report #GP before the unusable SS can cause an operand fault.
        image.cpu = cpu;
        assert_eq!(
            engine.observe(
                TestModule::interpreter_with_profile(profile),
                &image.input(),
                1
            ),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }],
            ),
            "{profile:?} interpreter fallback preserves prefix progress",
        );
    }
}

#[test]
fn a_valid_block_prefix_retires_before_interpreter_fetch_faults() {
    valid_prefix_then_fetch_fault(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_a_valid_block_prefix_retires_before_interpreter_fetch_faults() {
    valid_prefix_then_fetch_fault(Engine::V8);
}
