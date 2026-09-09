use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{both, check, Exit, Image, Step},
    step::TestModule,
};

#[test]
fn stack_encodings_consume_only_their_register_address_or_immediate_fields() {
    for code in [
        &[0x50][..],
        &[0x57],
        &[0x58],
        &[0x5f],
        &[0x66, 0x54],
        &[0x66, 0x66, 0x5c],
        &[0x68, 0x50, 0x58, 0x68, 0x6a],
        &[0x66, 0x68, 0x8f, 0xff],
        &[0x6a, 0x80],
        &[0x66, 0x6a, 0xff],
        &[0xff, 0xf4],
        &[0x66, 0xff, 0xf7],
        &[0x8f, 0xc4],
        &[0x66, 0x8f, 0xc7],
        &[0xff, 0x34, 0x24],
        &[0x66, 0xff, 0x74, 0x8c, 0x80],
        &[0xff, 0xb4, 0x25, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0xff, 0x35, 0x11, 0x22, 0x33, 0x44],
        &[0x8f, 0x04, 0x24],
        &[0x66, 0x8f, 0x44, 0x8c, 0x80],
        &[0x8f, 0x84, 0x25, 0x11, 0x22, 0x33, 0x44],
        &[0x66, 0x8f, 0x05, 0x11, 0x22, 0x33, 0x44],
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
                .bytes,
            "{code:02x?}",
        );
    }
}

#[test]
fn unsupported_stack_group_extensions_stop_before_address_fetch() {
    let extensions = (1..8).map(|extension| (0x8f, (extension << 3) | 4)).chain([
        (0xff, 0x14),
        (0xff, 0x1c),
        (0xff, 0x24),
        (0xff, 0x2c),
        (0xff, 0x3c),
    ]);
    for (opcode, modrm) in extensions {
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
                "an unsupported extension does not fetch its missing SIB",
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
fn missing_stack_instruction_fields_fault_before_any_stack_access() {
    for code in [
        &[0x68, 0x78, 0x56, 0x34][..],
        &[0x66, 0x68, 0x78],
        &[0x6a],
        &[0x66, 0x6a],
        &[0xff],
        &[0xff, 0x34],
        &[0x8f],
        &[0x8f, 0x04],
        &[0x66, 0x8f, 0x84, 0x25, 0, 0x40, 0],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = Image::new(&[]);
        image.cpu.flags.kind = 0xff;
        image.cpu.registers.esp = 0x4000;
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "the required immediate or address field crosses an unmapped code page",
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
}

#[test]
fn stack_encodings_reject_a_required_sixteenth_byte_before_fetching_it() {
    for suffix in [
        &[0x68, 1][..],
        &[0x6a],
        &[0xff],
        &[0x8f, 0x04],
        &[0x8f, 0x84, 0x25, 0, 0x40],
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
            "the length limit precedes the missing next code page",
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
fn all_stack_forms_can_end_at_byte_fifteen_with_repeated_word_prefixes() {
    struct Case {
        suffix: &'static [u8],
        stack_pointer: u32,
        eax: u32,
        ram: &'static [(u32, &'static [u8])],
    }
    for case in [
        Case {
            suffix: &[0x50],
            stack_pointer: 0x9004,
            eax: 0x1111_1111,
            ram: &[(0x8002, &[0x11, 0x11])],
        },
        Case {
            suffix: &[0x58],
            stack_pointer: 0x9000,
            eax: 0x1111_5678,
            ram: &[],
        },
        Case {
            suffix: &[0x68, 0x80, 0xff],
            stack_pointer: 0x9004,
            eax: 0x1111_1111,
            ram: &[(0x8002, &[0x80, 0xff])],
        },
        Case {
            suffix: &[0x6a, 0x80],
            stack_pointer: 0x9004,
            eax: 0x1111_1111,
            ram: &[(0x8002, &[0x80, 0xff])],
        },
        Case {
            suffix: &[0xff, 0xf4],
            stack_pointer: 0x9004,
            eax: 0x1111_1111,
            ram: &[(0x8002, &[0x04, 0x90])],
        },
        Case {
            suffix: &[0x8f, 0xc0],
            stack_pointer: 0x9000,
            eax: 0x1111_5678,
            ram: &[],
        },
        Case {
            suffix: &[0xff, 0x34, 0x24],
            stack_pointer: 0x9004,
            eax: 0x1111_1111,
            ram: &[(0x8002, &[0xbc, 0x9a])],
        },
        Case {
            suffix: &[0x8f, 0x04, 0x24],
            stack_pointer: 0x9000,
            eax: 0x1111_1111,
            ram: &[(0x8002, &[0x78, 0x56])],
        },
    ] {
        let code = [vec![0x66; 15 - case.suffix.len()], case.suffix.to_vec()].concat();
        let mut image = Image::new(&[]);
        image.cpu.flags.kind = 0xff;
        image.cpu.registers.esp = case.stack_pointer;
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        image.map(9, 0x8000, true);
        image.data(0x8000, &[0x78, 0x56, 0xa5, 0xa5, 0xbc, 0x9a, 0x5a]);
        let mut expected_cpu = image.cpu;
        expected_cpu.registers.esp = 0x9002;
        expected_cpu.registers.eax = case.eax;
        expected_cpu.eip = 0x2000;
        expected_cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "repeated 66 selects word width and no byte sixteen is fetched",
            &code,
            1,
            &image,
            &[Step {
                cpu: expected_cpu,
                ram: case.ram,
                exit: Exit::Dispatch(0x2000),
            }],
        );
    }
}
