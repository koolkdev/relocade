use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, compile_block_from_bytes_with_profile, BlockError, SegmentAttributes,
    SegmentProfile, StoredSegment,
};

#[test]
fn default_block_entry_matches_explicit_flat_compilation() {
    for bytes in [
        &[0x90][..],
        &[0x67, 0x8b, 0x00],
        &[0x50, 0x5b],
        &[0x67, 0xf3, 0xa4],
    ] {
        let default = compile_block_from_bytes(0x1000, bytes, 1).unwrap();
        let explicit =
            compile_block_from_bytes_with_profile(0x1000, bytes, 1, SegmentProfile::Flat32)
                .unwrap();
        assert_eq!(default.bytes, explicit.bytes);
        assert_eq!(default.entry, explicit.entry);
        assert_eq!(default.segment_profile, Some(SegmentProfile::Flat32));
        assert_eq!(explicit.segment_profile, default.segment_profile);
    }
}

#[test]
fn snapshot_field_bounds_follow_the_selected_code_defaults() {
    let word = [0xb8, 0x34, 0x12];
    let module =
        compile_block_from_bytes_with_profile(0x1000, &word, 1, SegmentProfile::Segmented16)
            .unwrap();
    assert_eq!(module.segment_profile, Some(SegmentProfile::Segmented16));
    assert!(matches!(
        compile_block_from_bytes_with_profile(0x1000, &word, 1, SegmentProfile::Segmented32),
        Err(BlockError::TruncatedInstruction {
            address: 0x1000,
            available: 3
        }),
    ));
    assert!(matches!(
        compile_block_from_bytes_with_profile(
            0x1000,
            &[0x66, 0xb8, 0x34, 0x12],
            1,
            SegmentProfile::Segmented16
        ),
        Err(BlockError::TruncatedInstruction {
            address: 0x1000,
            available: 4
        }),
    ));
    let overlong = [vec![0x66; 12], vec![0xb8, 0x78, 0x56, 0x34, 0x12]].concat();
    assert!(matches!(
        compile_block_from_bytes_with_profile(0x1000, &overlong, 1, SegmentProfile::Segmented16),
        Err(BlockError::InstructionTooLong { address: 0x1000 }),
    ));
    for profile in [SegmentProfile::Segmented16, SegmentProfile::Segmented32] {
        for prefix in [&[0xeb, 0][..], &[0xf3, 0xa4]] {
            let bytes = [prefix, &[0x0f]].concat();
            assert!(compile_block_from_bytes_with_profile(0x1000, &bytes, 5, profile).is_ok());
        }
        assert!(matches!(
            compile_block_from_bytes_with_profile(0x1000, &[0x90], 0, profile),
            Err(BlockError::ZeroInstructionLimit),
        ));
    }
}

fn runtime_stack_attributes(engine: Engine) {
    let bytes = [0x50, 0x5b];
    let compiled =
        compile_block_from_bytes_with_profile(0x1000, &bytes, 2, SegmentProfile::Segmented32)
            .unwrap();
    let block = TestModule::new(&compiled);
    for (big, page) in [(false, 0xc), (true, 0x1234c)] {
        let mut image = Image::new(&bytes);
        image.cpu.registers.eax = 0x1234_5678;
        image.cpu.registers.esp = 0x1234_8004;
        image.cpu.segments.ss = StoredSegment {
            base: 0x4000,
            attributes: SegmentAttributes::from_bits(if big { 0x15 } else { 0x05 }),
            ..StoredSegment::flat_data32(0x23)
        };
        image.map(page, 0x8000, true);
        assert!(SegmentProfile::Segmented32.is_compatible_with(&image.cpu.segments));
        let mut cpu = image.cpu;
        cpu.registers.ebx = 0x1234_5678;
        cpu.eip = 0x1002;
        cpu.instruction_count = 1;
        assert_eq!(
            engine.observe(&block, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x8000, &[0x78, 0x56, 0x34, 0x12])],
                    exit: Exit::Dispatch(0x1002),
                }]
            ),
            "SS.B={big}",
        );
    }
}

#[test]
fn a_segmented_block_reads_stack_attributes_at_each_entry() {
    runtime_stack_attributes(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_a_segmented_block_reads_stack_attributes_at_each_entry() {
    runtime_stack_attributes(Engine::V8);
}
