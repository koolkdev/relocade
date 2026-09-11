use wasm86_x86::Gpr32;

use crate::support::cases::{
    test_cases, InstructionCase,
    Permissions::{ReadOnly, ReadWrite},
};

use super::{other_register_inputs, Operation, OPERATIONS, STORED_FLAGS};

fn unchanged_modifiers_on_read_only_memory() -> Vec<InstructionCase> {
    let mut cases = Vec::new();
    for operation in [Operation::Bts, Operation::Btr, Operation::Btc] {
        for bits in [16, 32] {
            for input in [0_u32, u32::MAX] {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, 0xba, 0x03 | (operation.extension() << 3), 0]);
                cases.push(
                    InstructionCase::preserving_flags(
                        format!("{operation:?} {bits}-bit {input:x} requires a writable operand"),
                        &code,
                    )
                    .initial_registers(&other_register_inputs(&[Gpr32::Ebx]))
                    .stored_flags(STORED_FLAGS)
                    .initial_register(Gpr32::Ebx, 0x4000)
                    .map_page(4, 0x8000, ReadOnly)
                    .backing(0x7fff, &[0x5a])
                    .backing(0x8000, &input.to_le_bytes())
                    .backing(0x8004, &[0x5a])
                    .fault(0x4000, 3),
                );
            }
        }
    }
    cases
}

test_cases!(
    unchanged_modifiers_still_require_write_access,
    unchanged_modifiers_on_read_only_memory()
);

fn adjusted_operand_faults() -> Vec<InstructionCase> {
    struct Operand {
        name: &'static str,
        bits: u32,
        base: u32,
        index: u32,
        immediate: bool,
        address: u32,
        first_writable: Option<bool>,
        second_writable: Option<bool>,
        extra_read_only_page: Option<u32>,
        read_fault: Option<u32>,
        write_fault: u32,
        write_error: u16,
    }
    let mut cases = Vec::new();
    for operand in [
        Operand {
            name: "missing selected word",
            bits: 16,
            base: 0x4000,
            index: 0,
            immediate: true,
            address: 0x4000,
            first_writable: None,
            second_writable: None,
            extra_read_only_page: None,
            read_fault: Some(0x4000),
            write_fault: 0x4000,
            write_error: 2,
        },
        Operand {
            name: "adjusted dword misses despite a present encoded base",
            bits: 32,
            base: 0x4ffc,
            index: 32,
            immediate: false,
            address: 0x5000,
            first_writable: None,
            second_writable: None,
            extra_read_only_page: Some(4),
            read_fault: Some(0x5000),
            write_fault: 0x5000,
            write_error: 2,
        },
        Operand {
            name: "word signed index ignores the parent register's high word",
            bits: 16,
            base: 0x5000,
            index: 0x4321_8000,
            immediate: false,
            address: 0x4000,
            first_writable: None,
            second_writable: None,
            extra_read_only_page: Some(5),
            read_fault: Some(0x4000),
            write_fault: 0x4000,
            write_error: 2,
        },
        Operand {
            name: "bit in first byte still requires the full word",
            bits: 16,
            base: 0x4fff,
            index: 0,
            immediate: true,
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: None,
            extra_read_only_page: None,
            read_fault: Some(0x5000),
            write_fault: 0x5000,
            write_error: 2,
        },
        Operand {
            name: "negative dword index reaches a missing second page",
            bits: 32,
            base: 0x5002,
            index: u32::MAX,
            immediate: false,
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: None,
            extra_read_only_page: None,
            read_fault: Some(0x5000),
            write_fault: 0x5000,
            write_error: 2,
        },
        Operand {
            name: "word second page must be writable for modifiers",
            bits: 16,
            base: 0x5001,
            index: 0xffff,
            immediate: false,
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: Some(false),
            extra_read_only_page: None,
            read_fault: None,
            write_fault: 0x5000,
            write_error: 3,
        },
        Operand {
            name: "dword second page must be writable for modifiers",
            bits: 32,
            base: 0x5002,
            index: u32::MAX,
            immediate: false,
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: Some(false),
            extra_read_only_page: None,
            read_fault: None,
            write_fault: 0x5000,
            write_error: 3,
        },
        Operand {
            name: "write fails on the first page before a missing second page",
            bits: 16,
            base: 0x5001,
            index: 0xffff,
            immediate: false,
            address: 0x4fff,
            first_writable: Some(false),
            second_writable: None,
            extra_read_only_page: None,
            read_fault: Some(0x5000),
            write_fault: 0x4fff,
            write_error: 3,
        },
        Operand {
            name: "immediate high bits cannot bypass the split operand",
            bits: 16,
            base: 0x4fff,
            index: 255,
            immediate: true,
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: None,
            extra_read_only_page: None,
            read_fault: Some(0x5000),
            write_fault: 0x5000,
            write_error: 2,
        },
        Operand {
            name: "adjusted word span cannot wrap",
            bits: 16,
            base: 1,
            index: 0xffff,
            immediate: false,
            address: u32::MAX,
            first_writable: Some(true),
            second_writable: None,
            extra_read_only_page: None,
            read_fault: Some(u32::MAX),
            write_fault: u32::MAX,
            write_error: 2,
        },
        Operand {
            name: "adjusted dword span cannot wrap",
            bits: 32,
            base: 2,
            index: u32::MAX,
            immediate: false,
            address: 0xffff_fffe,
            first_writable: Some(true),
            second_writable: None,
            extra_read_only_page: None,
            read_fault: Some(0xffff_fffe),
            write_fault: 0xffff_fffe,
            write_error: 2,
        },
    ] {
        for operation in OPERATIONS {
            let (address, error) = if operation.modifies() {
                (operand.write_fault, operand.write_error)
            } else if let Some(address) = operand.read_fault {
                (address, 0)
            } else {
                continue;
            };
            let mut code = if operand.bits == 16 {
                vec![0x66]
            } else {
                vec![]
            };
            if operand.immediate {
                code.extend_from_slice(&[
                    0x0f,
                    0xba,
                    0x03 | (operation.extension() << 3),
                    operand.index as u8,
                ]);
            } else {
                code.extend_from_slice(&[0x0f, operation.register_opcode(), 0x13]);
            }
            let mut case = InstructionCase::preserving_flags(
                format!("{operation:?}: {}", operand.name),
                &code,
            )
            .initial_registers(&other_register_inputs(&[Gpr32::Ebx, Gpr32::Edx]))
            .stored_flags(STORED_FLAGS)
            .initial_register(Gpr32::Ebx, operand.base)
            .initial_register(Gpr32::Edx, operand.index)
            .backing(0x8000, &[0x81, 0x80, 0xff, 0xff])
            .backing(0x8ffc, &[0x5a, 0x78, 0x56, 0x34])
            .backing(0xa000, &[0x12, 0x5a])
            .backing(0xc000, &[0xde, 0xad, 0xbe, 0xef])
            .fault(address, error);
            if let Some(page) = operand.extra_read_only_page {
                case = case.map_page(page, 0xc000, ReadOnly);
            }
            if let Some(writable) = operand.first_writable {
                case = case.map_page(
                    operand.address >> 12,
                    0x8000,
                    if writable { ReadWrite } else { ReadOnly },
                );
            }
            if let Some(writable) = operand.second_writable {
                case = case.map_page(
                    (operand.address >> 12) + 1,
                    0xa000,
                    if writable { ReadWrite } else { ReadOnly },
                );
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(
    faults_check_the_entire_adjusted_operand,
    adjusted_operand_faults()
);
