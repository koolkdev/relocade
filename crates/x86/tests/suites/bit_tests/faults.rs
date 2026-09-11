use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{image, operand_address, Operation, OPERATIONS};

#[test]
fn even_unchanged_modifiers_require_write_permission_before_publishing_carry() {
    for operation in [Operation::Bts, Operation::Btr, Operation::Btc] {
        for bits in [16, 32] {
            for value in [0_u32, u32::MAX] {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, 0xba, 0x03 | (operation.extension() << 3), 0]);
                let mut image = image(&code);
                image.cpu.registers.ebx = 0x4000;
                image.map(4, 0x8000, false);
                image.data(0x7fff, &[0x5a]);
                image.data(0x8000, &value.to_le_bytes());
                image.data(0x8004, &[0x5a]);
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?} {bits}-bit {value:x} requires a writable operand"),
                    &code,
                    1,
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::PageFault {
                            address: 0x4000,
                            error: 3,
                        },
                    }],
                );
            }
        }
    }
}

#[test]
fn bit_memory_faults_check_the_adjusted_full_operand_before_any_effect() {
    struct Case {
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
    for case in [
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        Case {
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
        assert_eq!(
            operand_address(case.base, case.bits, case.index, case.immediate),
            case.address,
            "{}",
            case.name
        );
        for operation in OPERATIONS {
            let exit = if operation.modifies() {
                Exit::PageFault {
                    address: case.write_fault,
                    error: case.write_error,
                }
            } else if let Some(address) = case.read_fault {
                Exit::PageFault { address, error: 0 }
            } else {
                continue;
            };
            let mut code = if case.bits == 16 { vec![0x66] } else { vec![] };
            if case.immediate {
                code.extend_from_slice(&[
                    0x0f,
                    0xba,
                    0x03 | (operation.extension() << 3),
                    case.index as u8,
                ]);
            } else {
                code.extend_from_slice(&[0x0f, operation.register_opcode(), 0x13]);
            }
            let mut image = image(&code);
            image.cpu.registers.ebx = case.base;
            image.cpu.registers.edx = case.index;
            if let Some(page) = case.extra_read_only_page {
                image.map(page, 0xc000, false);
            }
            if let Some(writable) = case.first_writable {
                image.map(case.address >> 12, 0x8000, writable);
            }
            if let Some(writable) = case.second_writable {
                image.map((case.address >> 12) + 1, 0xa000, writable);
            }
            image.data(0x8000, &[0x81, 0x80, 0xff, 0xff]);
            image.data(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]);
            image.data(0xa000, &[0x12, 0x5a]);
            image.data(0xc000, &[0xde, 0xad, 0xbe, 0xef]);
            both(
                TestModule::interpreter(),
                &format!("{operation:?}: {}", case.name),
                &code,
                1,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit,
                }],
            );
        }
    }
}
