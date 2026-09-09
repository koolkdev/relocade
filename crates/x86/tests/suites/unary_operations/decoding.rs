use wasm86_x86::{compile_block_from_bytes, BlockError, StatusFlags};

use crate::support::{
    machine::{both, check, Exit, Image, Step},
    step::TestModule,
};

#[test]
fn unary_lengths_stop_after_the_selected_register_or_address() {
    for code in [
        &[0x40][..],
        &[0x4f],
        &[0x66, 0x43],
        &[0x66, 0x66, 0x4c],
        &[0xfe, 0xc4],
        &[0x66, 0xfe, 0xcc],
        &[0xff, 0xc0],
        &[0x66, 0xff, 0xc8],
        &[0xf6, 0xd0],
        &[0x66, 0xf6, 0xdc],
        &[0xf7, 0xd0],
        &[0x66, 0xf7, 0xd8],
        &[0xfe, 0x44, 0x8b, 0x80],
        &[0xff, 0x84, 0x8b, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0xff, 0x0d, 0x11, 0x22, 0x33, 0x44],
        &[0xf6, 0x54, 0x8b, 0x80],
        &[0xf7, 0x14, 0x25, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0xf7, 0x9c, 0x8b, 0x11, 0x22, 0x33, 0x44],
        // The same F6/F7 opcodes still consume an immediate for TEST /0.
        &[0xf6, 0xc0, 0xf7],
        &[0xf7, 0xc0, 0xf6, 0xf7, 0xfe, 0xff],
        &[0x66, 0xf7, 0xc0, 0xf6, 0xf7],
    ] {
        for available in 0..code.len() {
            assert_eq!(
                compile_block_from_bytes(0x1000, &code[..available], 1).err(),
                Some(BlockError::TruncatedInstruction {
                    address: 0x1000,
                    available
                }),
                "{code:02x?}, available {available}",
            );
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let with_suffix = [code, &[0x0f]].concat();
        assert_eq!(
            complete.bytes,
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes
        );
    }
}

#[test]
fn mixed_test_not_and_neg_encodings_consume_only_their_own_fields() {
    let code = [
        0xf6, 0xc0, 0xf7, // TEST AL, 0xf7
        0xf6, 0xd0, // NOT AL
        0xf6, 0xd8, // NEG AL
        0xf7, 0xc0, 0xf6, 0xf7, 0xfe, 0xff, // TEST EAX, 0xfffe_f7f6
        0xf7, 0xd0, // NOT EAX
        0xf7, 0xd8, // NEG EAX
    ];
    let mut image = Image::new(&code);
    image.cpu.flags.kind = 0xff;
    image.cpu.registers.eax = 0x1234_5678;
    let mut expected_cpu = image.cpu;
    let mut steps = Vec::new();

    expected_cpu.flags.kind = 3;
    expected_cpu.flags.left = 0x70;
    expected_cpu.eip = 0x1003;
    expected_cpu.instruction_count = 0;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1003),
    });

    expected_cpu.registers.eax = 0x1234_5687;
    expected_cpu.eip = 0x1005;
    expected_cpu.instruction_count = 1;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1005),
    });

    expected_cpu.registers.eax = 0x1234_5679;
    expected_cpu.flags.kind = 1;
    expected_cpu.flags.left = 0;
    expected_cpu.flags.right = 0x87;
    expected_cpu.eip = 0x1007;
    expected_cpu.instruction_count = 2;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1007),
    });

    expected_cpu.flags.kind = 11;
    expected_cpu.flags.left = 0x1234_5670;
    expected_cpu.eip = 0x100d;
    expected_cpu.instruction_count = 3;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100d),
    });

    expected_cpu.registers.eax = 0xedcb_a986;
    expected_cpu.eip = 0x100f;
    expected_cpu.instruction_count = 4;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x100f),
    });

    expected_cpu.registers.eax = 0x1234_567a;
    expected_cpu.flags.kind = 9;
    expected_cpu.flags.left = 0;
    expected_cpu.flags.right = 0xedcb_a986;
    expected_cpu.eip = 0x1011;
    expected_cpu.instruction_count = 5;
    steps.push(Step {
        cpu: expected_cpu,
        ram: &[],
        exit: Exit::Dispatch(0x1011),
    });

    both(
        TestModule::interpreter(),
        "mixed F6/F7 forms",
        &code,
        6,
        &image,
        &steps,
    );
}

#[test]
fn unsupported_group_extensions_stop_before_sib_or_displacement_fetch() {
    for (opcode, modrm) in [(0xfe, 0x14), (0xff, 0x3c), (0xf6, 0x0c), (0xf7, 0x3d)] {
        for prefixes in [0, 13] {
            let code = [vec![0x66; prefixes], vec![opcode, modrm]].concat();
            let start = 0x2000 - code.len() as u32;
            assert_eq!(
                compile_block_from_bytes(start, &code, 1).err(),
                Some(BlockError::UnsupportedInstruction {
                    address: start,
                    opcode
                }),
            );
            let mut image = Image::new(&[]);
            image.cpu.flags.kind = 0xff;
            image.cpu.eip = start;
            image.data(0x3000 + (start & 0xfff), &code);
            check(
                TestModule::interpreter(),
                "unsupported extension precedes address fields",
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(
                        0x0008_0000_0000_0000 | (u64::from(opcode) << 32) | u64::from(start),
                    ),
                }],
            );
        }
    }
}

#[test]
fn required_unary_fields_fault_before_data_or_old_carry_access() {
    for code in [
        &[0xfe][..],
        &[0xff, 0x04],
        &[0xf6, 0x14],
        &[0x66, 0xf7, 0x9c, 0x25, 0, 0x40, 0],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = Image::new(&[]);
        image.cpu.flags.kind = 0xff;
        image.cpu.registers.ebx = 0x4000;
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "required unary field crosses an unmapped code page",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            }],
        );
    }
    for suffix in [
        &[0xfe][..],
        &[0xff, 0x04],
        &[0xf7, 0x94, 0x25],
        &[0xf7, 0xc0, 1],
    ] {
        let code = [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat();
        assert_eq!(
            compile_block_from_bytes(0x1ff1, &code, 1).err(),
            Some(BlockError::InstructionTooLong { address: 0x1ff1 }),
        );
        let mut image = Image::new(&[]);
        image.cpu.flags.kind = 0xff;
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            TestModule::interpreter(),
            "byte sixteen is rejected before page fetch",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0002_0000_0000_0000),
            }],
        );
    }
}

#[test]
fn unary_instructions_can_end_at_byte_fifteen_without_fetching_an_immediate() {
    for (suffix, result, kind) in [
        (&[0x40][..], 0x1234_0000, 0),          // Repeated 66 selects INC AX.
        (&[0xfe, 0xc0][..], 0x1234_ff00, 0),    // Byte INC ignores 66.
        (&[0xf6, 0xd0][..], 0x1234_ff00, 0xff), // Byte NOT ignores 66.
        (&[0xf7, 0xd8][..], 0x1234_0001, 5),    // NEG AX has no immediate.
    ] {
        let code = [vec![0x66; 15 - suffix.len()], suffix.to_vec()].concat();
        let mut image = Image::new(&[]);
        image.cpu.flags.kind = if kind == 0 { 0 } else { 0xff };
        image.cpu.flags.status.cf = 1;
        image.cpu.registers.eax = 0x1234_ffff;
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        let mut expected_cpu = image.cpu;
        expected_cpu.registers.eax = result;
        expected_cpu.flags.kind = kind;
        if kind == 0 {
            expected_cpu.flags.status = StatusFlags {
                cf: 1,
                pf: 1,
                af: 1,
                zf: 1,
                sf: 0,
                of: 0,
            };
        } else if kind == 5 {
            expected_cpu.flags.left = 0;
            expected_cpu.flags.right = 0xffff;
        }
        expected_cpu.eip = 0x2000;
        expected_cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "unary instruction ends at byte fifteen",
            &code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: &[],
                exit: Exit::Dispatch(0x2000),
            }],
        );
    }
}
