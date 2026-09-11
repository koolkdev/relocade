use crate::support::cases::{test_cases, InstructionCase as Case};
use wasm86_x86::Gpr32::{Eax, Ebx};
use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    arithmetic,
    machine::{check, Exit, Step},
    step::TestModule,
};

#[test]
fn register_only_modrm_forms_are_unsupported_without_extra_fetches() {
    for (code, start, exit) in [
        (&[0x8d, 0xc4][..], 0x1ffe, 0x0008_008d_0000_1ffe),
        (&[0x66, 0x8d, 0xfd][..], 0x1ffd, 0x0008_008d_0000_1ffd),
    ] {
        assert!(matches!(
            compile_block_from_bytes(start, code, 1),
            Err(BlockError::UnsupportedInstruction { address, opcode: 0x8d }) if address == start
        ));
        let mut image = arithmetic::image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "LEA rejects mod=3 at the final mapped byte",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(exit),
            }],
        );
    }
}

#[test]
fn snapshots_require_address_fields_but_no_immediate_or_successor() {
    for code in [
        &[0x8d, 0x03][..],
        &[0x66, 0x8d, 0x44, 0x8b, 0x80][..],
        &[0x8d, 0x04, 0x25, 0x78, 0x56, 0x34, 0x12][..],
    ] {
        for available in 0..code.len() {
            assert!(
                matches!(
                    compile_block_from_bytes(0x1000, &code[..available], 1),
                    Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                        if actual == available
                ),
                "{code:02x?}, available {available}",
            );
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let with_suffix = [code, &[0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            complete.bytes,
        );
    }
}

#[test]
fn operand_prefixes_count_toward_the_instruction_length_limit() {
    for (prefixes, suffix) in [
        (14, &[0x8d][..]),
        (13, &[0x8d, 0x04][..]),
        (12, &[0x8d, 0x44, 0x0b][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 }),
        ));
        let mut image = arithmetic::image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            TestModule::interpreter(),
            "required LEA address byte is beyond the instruction limit",
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
fn required_address_bytes_can_fault_during_instruction_fetch() {
    for code in [
        &[0x8d][..],
        &[0x66, 0x8d, 0x04][..],
        &[0x8d, 0x44, 0x8b][..],
        &[0x8d, 0x04, 0x25, 0x20, 0x40, 0][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = arithmetic::image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "LEA still fetches each required encoding byte",
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
fn completed_lea_is_published_before_a_later_fetch_fault() {
    let mut image = arithmetic::image(&[]);
    image.cpu.eip = 0x1ffd;
    image.data(0x3ffd, &[0x8d, 0x03, 0x8d]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4444_4444;
    cpu.eip = 0x1fff;
    cpu.instruction_count = 0;
    check(
        TestModule::interpreter(),
        "LEA retires before the next instruction's missing ModRM",
        &image,
        &[
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
            Step {
                cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: 0x2000,
                    error: 0x10,
                },
            },
        ],
    );
}

fn maximum_length_cases() -> Vec<Case> {
    let code = [vec![0x66; 13], vec![0x8d, 0x03]].concat();
    vec![Case::preserving_flags(
        "fifteen-byte LEA finishes before its unmapped successor",
        &code,
    )
    .initial_register(Ebx, 0x8123_5678)
    .register(Eax, 0x1111_1111, 0x1111_5678)
    .at(0x1ff1)]
}
test_cases!(fifteen_byte_lea, maximum_length_cases());
