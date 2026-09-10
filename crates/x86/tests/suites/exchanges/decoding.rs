use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::image;

#[test]
fn snapshots_require_each_form_field_but_no_immediate_or_successor() {
    let mut encodings = vec![
        vec![0x86, 0xc4],
        vec![0x87, 0xc1],
        vec![0x66, 0x87, 0xf5],
        vec![0x86, 0x44, 0x8b, 0x80],
        vec![0x66, 0x87, 0x04, 0x25, 0x20, 0x40, 0, 0],
        vec![0x87, 0x05, 0x20, 0x40, 0, 0],
    ];
    for opcode in 0x90..=0x97 {
        encodings.push(vec![opcode]);
        encodings.push(vec![0x66, opcode]);
    }
    for code in encodings {
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
        let complete = compile_block_from_bytes(0x1000, &code, 1).unwrap();
        let with_suffix = [code.as_slice(), &[0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            complete.bytes,
        );
    }
}

#[test]
fn fifteen_byte_exchanges_finish_without_fetching_their_successor() {
    for (prefixes, suffix, eax, ecx) in [
        (14, &[0x90][..], 0x4433_2211, 0x8877_6655),
        (13, &[0x86, 0xc4][..], 0x4433_1122, 0x8877_6655),
        (13, &[0x87, 0xc1][..], 0x4433_6655, 0x8877_2211),
    ] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        let mut image = image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        let mut cpu = image.cpu;
        cpu.registers.eax = eax;
        cpu.registers.ecx = ecx;
        cpu.eip = 0x2000;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "fifteen-byte XCHG ends at the final mapped byte",
            &code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(0x2000),
            }],
        );
    }
}

#[test]
fn instruction_length_limit_precedes_fetching_a_required_sixteenth_byte() {
    let mut encodings = vec![vec![0x66; 15]];
    for opcode in [0x86, 0x87] {
        for (prefixes, suffix) in [
            (14, vec![opcode]),
            (13, vec![opcode, 0x04]),
            (12, vec![opcode, 0x44, 0x0b]),
            (11, vec![opcode, 0x05, 0x20, 0x40]),
        ] {
            encodings.push([vec![0x66; prefixes], suffix].concat());
        }
    }
    for code in encodings {
        assert!(
            matches!(
                compile_block_from_bytes(0x1ff1, &code, 1),
                Err(BlockError::InstructionTooLong { address: 0x1ff1 }),
            ),
            "{code:02x?}",
        );
        let mut image = image(&[]);
        image.cpu.eip = 0x1ff1;
        image.data(0x3ff1, &code);
        check(
            TestModule::interpreter(),
            "required XCHG encoding byte is beyond the instruction limit",
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
fn required_encoding_bytes_can_fault_during_instruction_fetch() {
    for code in [
        &[0x66][..],
        &[0x86][..],
        &[0x87][..],
        &[0x66, 0x87][..],
        &[0x66, 0x86, 0x04][..],
        &[0x87, 0x44, 0x8b][..],
        &[0x86, 0x04, 0x25, 0x20, 0x40, 0][..],
        &[0x87, 0x05, 0x20, 0x40, 0][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "XCHG fetches each required encoding byte",
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
fn both_exchange_destinations_are_published_before_a_later_fetch_fault() {
    let mut image = image(&[]);
    image.cpu.eip = 0x1ffd;
    image.data(0x3ffd, &[0x87, 0xc1, 0x86]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x8877_6655;
    cpu.registers.ecx = 0x4433_2211;
    cpu.eip = 0x1fff;
    cpu.instruction_count = 0;
    check(
        TestModule::interpreter(),
        "XCHG retires before the next instruction's missing ModRM",
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
