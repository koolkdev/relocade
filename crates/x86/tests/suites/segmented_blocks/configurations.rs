use crate::support::config::{FLAT32, SEGMENTED16, SEGMENTED32};
use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::TestModule,
};
use relocade_x86::{compile_block_from_bytes, BlockError, Compiler, SegmentRegister};

#[test]
fn default_block_entry_matches_explicit_flat_compilation() {
    for bytes in [
        &[0x90][..],
        &[0x67, 0x8b, 0x00],
        &[0x50, 0x5b],
        &[0x67, 0xf3, 0xa4],
    ] {
        let default = compile_block_from_bytes(0x1000, bytes, 1).unwrap();
        let explicit = Compiler::new(FLAT32)
            .compile_block(0x1000, bytes, 1)
            .unwrap();
        assert_eq!(default.bytes, explicit.bytes);
        assert_eq!(default.entry, explicit.entry);
        assert_eq!(default.config, Some(FLAT32));
        assert_eq!(explicit.config, default.config);
    }
}

#[test]
fn snapshot_field_bounds_follow_the_selected_code_defaults() {
    let word = [0xb8, 0x34, 0x12];
    let module = Compiler::new(SEGMENTED16)
        .compile_block(0x1000, &word, 1)
        .unwrap();
    assert_eq!(module.config, Some(SEGMENTED16));
    assert!(matches!(
        Compiler::new(SEGMENTED32).compile_block(0x1000, &word, 1),
        Err(BlockError::TruncatedInstruction {
            address: 0x1000,
            available: 3
        }),
    ));
    assert!(matches!(
        Compiler::new(SEGMENTED16).compile_block(0x1000, &[0x66, 0xb8, 0x34, 0x12], 1),
        Err(BlockError::TruncatedInstruction {
            address: 0x1000,
            available: 4
        }),
    ));
    let overlong = [vec![0x66; 12], vec![0xb8, 0x78, 0x56, 0x34, 0x12]].concat();
    assert!(matches!(
        Compiler::new(SEGMENTED16).compile_block(0x1000, &overlong, 1),
        Err(BlockError::InstructionTooLong { address: 0x1000 }),
    ));
    for config in [SEGMENTED16, SEGMENTED32] {
        assert!(Compiler::new(config)
            .compile_block(0x1000, &[0xeb, 0, 0x0f], 5)
            .is_ok());
        assert!(matches!(
            Compiler::new(config).compile_block(0x1000, &[0xf3, 0xa4, 0x0f], 2),
            Err(BlockError::TruncatedInstruction {
                address: 0x1002,
                available: 1
            }),
        ));
        assert!(matches!(
            Compiler::new(config).compile_block(0x1000, &[0x90], 0),
            Err(BlockError::ZeroInstructionLimit),
        ));
    }
}

#[test]
fn a_segmented_block_reads_stack_attributes_at_each_entry() {
    let bytes = [0x50, 0x5b];
    let compiled = Compiler::new(SEGMENTED32)
        .compile_block(0x1000, &bytes, 2)
        .unwrap();
    let block = TestModule::new(&compiled);
    for (big, page) in [(false, 0xc), (true, 0x1234c)] {
        let mut image = Image::new(&bytes);
        image.cpu.registers.eax = 0x1234_5678;
        image.cpu.registers.esp = 0x1234_8004;
        image.cpu.segments.ss = SegmentRegister {
            base: 0x4000,
            big: u8::from(big),
            ..SegmentRegister::flat_data32(0x23)
        };
        image.map(page, 0x8000, true);
        assert!(image.cpu.segments.is_compatible_with(SEGMENTED32.entry));
        let mut cpu = image.cpu;
        cpu.registers.ebx = 0x1234_5678;
        cpu.eip = 0x1002;
        cpu.instruction_count = 1;
        assert_eq!(
            block.observe(&image.input(), 1),
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
