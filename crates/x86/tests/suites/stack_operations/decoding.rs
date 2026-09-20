use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadWrite};
use wasm86_x86::Gpr32::{Eax, Esp};
use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{check, Exit, Image, Step},
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
    let extensions = (1..8)
        .map(|extension| (0x8f, (extension << 3) | 4))
        .chain([(0xff, 0x1c), (0xff, 0x3c)]);
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
            image.cpu.flags.status_source.kind = 0xff;
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
        image.cpu.flags.status_source.kind = 0xff;
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
        image.cpu.flags.status_source.kind = 0xff;
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

#[rustfmt::skip]
fn maximum_length_cases() -> Vec<Case> {
    struct Form { suffix: &'static [u8], stack: u32, eax: u32, stored: Option<&'static [u8]> }
    [
        Form { suffix: &[0x50], stack: 0x9004, eax: 0x1111_1111, stored: Some(&[0x11, 0x11]) },
        Form { suffix: &[0x58], stack: 0x9000, eax: 0x1111_5678, stored: None },
        Form { suffix: &[0x68, 0x80, 0xff], stack: 0x9004, eax: 0x1111_1111, stored: Some(&[0x80, 0xff]) },
        Form { suffix: &[0x6a, 0x80], stack: 0x9004, eax: 0x1111_1111, stored: Some(&[0x80, 0xff]) },
        Form { suffix: &[0xff, 0xf4], stack: 0x9004, eax: 0x1111_1111, stored: Some(&[0x04, 0x90]) },
        Form { suffix: &[0x8f, 0xc0], stack: 0x9000, eax: 0x1111_5678, stored: None },
        Form { suffix: &[0xff, 0x34, 0x24], stack: 0x9004, eax: 0x1111_1111, stored: Some(&[0xbc, 0x9a]) },
        Form { suffix: &[0x8f, 0x04, 0x24], stack: 0x9000, eax: 0x1111_1111, stored: Some(&[0x78, 0x56]) },
    ].into_iter().map(|form| {
        let code = [vec![0x66; 15 - form.suffix.len()], form.suffix.to_vec()].concat();
        let mut case = Case::preserving_flags(format!("fifteen-byte stack instruction {:02x?}", form.suffix), &code)
            .at(0x1ff1).register(Esp, form.stack, 0x9002).register(Eax, 0x1111_1111, form.eax)
            .map_page(9, 0x8000, ReadWrite).memory(0x9000, &[0x78, 0x56, 0xa5, 0xa5, 0xbc, 0x9a, 0x5a], ReadWrite);
        if let Some(bytes) = form.stored { case = case.expect_memory(0x9002, bytes); }
        case
    }).collect()
}
test_cases!(all_stack_forms_at_byte_fifteen, maximum_length_cases());
