use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::image;

#[test]
fn snapshot_forms_require_their_encoding_but_no_successor() {
    for code in [
        &[0x0f, 0xc0, 0xe0][..],
        &[0x66, 0x0f, 0xc1, 0xd8][..],
        &[0x0f, 0xb0, 0xe3][..],
        &[0x66, 0x0f, 0xb1, 0x44, 0x8b, 0x80][..],
        &[0x0f, 0xc1, 0x04, 0x25, 0x20, 0x40, 0, 0][..],
    ] {
        for available in 0..code.len() {
            assert!(matches!(
                compile_block_from_bytes(0x1000, &code[..available], 1),
                Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                    if actual == available
            ));
        }
        let module = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let with_suffix = [code, &[0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            module.bytes,
        );
    }
}

#[test]
fn fifteen_byte_exchanges_retire_without_fetching_the_next_page() {
    for (opcode, modrm, eax, kind) in [
        (0xc1, 0xc0, 0x4433_4422, 6), // XADD AX,AX
        (0xb1, 0xd8, 0x4433_eedd, 5), // CMPXCHG AX,BX
    ] {
        let code = [vec![0x66; 12], vec![0x0f, opcode, modrm]].concat();
        let mut image = image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        let mut cpu = image.cpu;
        cpu.registers.eax = eax;
        cpu.flags.kind = kind;
        cpu.flags.left = 0x2211;
        cpu.flags.right = 0x2211;
        cpu.eip = 0x2000;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "fifteen-byte exchange arithmetic",
            &code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn length_limit_precedes_fetching_an_unavailable_field() {
    for (prefixes, suffix) in [
        (14, &[0x0f][..]),
        (13, &[0x0f, 0xc0][..]),
        (12, &[0x0f, 0xb1, 0x04][..]),
        (11, &[0x0f, 0xc1, 0x44, 0x8b][..]),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        assert!(matches!(
            compile_block_from_bytes(0x1ff1, &code, 1),
            Err(BlockError::InstructionTooLong { address: 0x1ff1 })
        ));
        let mut image = image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            TestModule::interpreter(),
            "a required exchange field would exceed fifteen bytes",
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
fn missing_encoding_bytes_fault_before_operand_effects() {
    for code in [
        &[0x0f][..],
        &[0x0f, 0xc0][..],
        &[0x66, 0x0f, 0xb1, 0x04][..],
        &[0x0f, 0xc1, 0x44, 0x8b][..],
        &[0x0f, 0xb1, 0x05, 0x20, 0x40, 0][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "required exchange encoding byte is unmapped",
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
fn completed_addition_survives_a_later_modrm_fetch_fault() {
    let mut image = image(&[]);
    image.cpu.eip = 0x1ffb;
    image.data(0x3ffb, &[0x0f, 0xc0, 0xe0, 0x0f, 0xb0]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x4433_1133;
    cpu.flags.kind = 2;
    cpu.flags.left = 0x11;
    cpu.flags.right = 0x22;
    cpu.eip = 0x1ffe;
    cpu.instruction_count = 0;
    check(
        TestModule::interpreter(),
        "CMPXCHG cannot fetch its ModRM after a completed XADD",
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
