use wasm86_x86::{compile_block_from_bytes, BlockError};

use crate::support::{
    machine::{both, check, Exit, Step},
    step::TestModule,
};

use super::{expected, image, Operation};

#[test]
fn snapshot_lengths_distinguish_implicit_cl_and_immediate_counts() {
    for code in [
        &[0xd0, 0xe4][..],
        &[0xd1, 0x2d, 0x20, 0x40, 0, 0][..],
        &[0xd2, 0xfc][..],
        &[0x66, 0xd3, 0xf9][..],
        &[0xc0, 0xed, 3][..],
        &[0x66, 0xc1, 0x64, 0x8b, 0xfc, 32][..],
    ] {
        for available in 0..code.len() {
            assert!(
                matches!(
                    compile_block_from_bytes(0x1000, &code[..available], 1),
                    Err(BlockError::TruncatedInstruction { address: 0x1000, available: actual })
                        if actual == available
                ),
                "{code:02x?}, available {available}"
            );
        }
        let complete = compile_block_from_bytes(0x1000, code, 1).unwrap();
        let with_suffix = [code, &[0x0f]].concat();
        assert_eq!(
            compile_block_from_bytes(0x1000, &with_suffix, 1)
                .unwrap()
                .bytes,
            complete.bytes
        );
    }
}

#[test]
fn implicit_and_cl_forms_end_without_fetching_an_immediate() {
    for (opcode, bits, count) in [(0xd0, 8, 1), (0xd1, 32, 1), (0xd2, 8, 32), (0xd3, 32, 32)] {
        let code = [opcode, 0xe0];
        let mut image = image(&[]);
        image.cpu.eip = 0x1ffe;
        image.cpu.registers.ecx = 0x8877_6620;
        image.data(0x3ffe, &code);
        let result = expected(Operation::Shl, bits, image.cpu.registers.eax, count);
        let mut cpu = image.cpu;
        cpu.registers.eax = if bits == 8 {
            0x4433_2200 | result.value
        } else {
            result.value
        };
        result.apply_flags(&mut cpu);
        cpu.eip = 0x2000;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            "non-immediate shift ends at the last mapped byte",
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
fn the_fifteenth_byte_can_supply_a_zero_immediate_count() {
    let code = [vec![0x66; 12], vec![0xc1, 0xe0, 32]].concat();
    let mut image = image(&[]);
    image.cpu.eip = 0x1ff1;
    image.data(0x3ff1, &code);
    let mut cpu = image.cpu;
    cpu.eip = 0x2000;
    cpu.instruction_count = 0;
    both(
        TestModule::interpreter(),
        "fifteen-byte SHL AX,32 preserves flags and does not fetch a successor",
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

#[test]
fn unsupported_groups_stop_before_address_or_count_bytes() {
    for (opcode, extension) in [
        (0xc0, 2),
        (0xc1, 3),
        (0xd0, 2),
        (0xd1, 3),
        (0xd2, 6),
        (0xd3, 6),
    ] {
        let code = [opcode, 0x04 | (extension << 3)]; // Missing SIB, and possibly an immediate.
        assert!(matches!(
            compile_block_from_bytes(0x1ffe, &code, 1),
            Err(BlockError::UnsupportedInstruction { address: 0x1ffe, opcode: actual }) if actual == opcode
        ));
        let mut image = image(&[]);
        image.cpu.eip = 0x1ffe;
        image.data(0x3ffe, &code);
        check(
            TestModule::interpreter(),
            "carry rotations and undocumented group six remain unsupported",
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::Other(0x0008_0000_0000_1ffe | (u64::from(opcode) << 32)),
            }],
        );
    }
}

#[test]
fn instruction_length_limit_precedes_fetching_a_required_field() {
    for (prefixes, suffix) in [
        (14, &[0xd0][..]),
        (13, &[0xc1, 0xe0][..]),
        (13, &[0xd2, 0x24][..]),
        (12, &[0xd3, 0x6c, 0x8b][..]),
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
            "a required shift field would exceed fifteen bytes",
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
fn required_fields_fault_before_operand_effects() {
    for code in [
        &[0xd0][..],
        &[0xd2, 0x64][..],
        &[0x66, 0xd3, 0x6c, 0x8b][..],
        &[0xc0, 0xe4][..],
        &[0xc1, 0x25, 0x20, 0x40, 0][..],
        &[0x66, 0xc1, 0x64, 0x8b, 0x80][..],
    ] {
        let start = 0x2000 - code.len() as u32;
        let mut image = image(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        check(
            TestModule::interpreter(),
            "missing shift encoding bytes precede data and flag effects",
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
