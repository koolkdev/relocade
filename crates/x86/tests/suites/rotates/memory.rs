use wasm86_x86::Gpr32;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, Operation, PRIOR_FLAGS};

#[test]
fn memory_rotates_use_exact_widths_and_old_address_registers() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        operation: Operation,
        bits: u32,
        count: u8,
        input: u32,
        bytes: &'static [u8],
        written: &'static [u8],
        address: u32,
        registers: &'static [(Gpr32, u32)],
    }
    for case in [
        Case {
            name: "implicit byte rotate needs only the last mapped byte",
            code: &[0xd0, 0x03],
            operation: Operation::Rol,
            bits: 8,
            count: 1,
            input: 0x81,
            bytes: &[0x81],
            written: &[3],
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "CL count shares the full ECX address base",
            code: &[0x66, 0xd3, 0x09],
            operation: Operation::Ror,
            bits: 16,
            count: 1,
            input: 0x8001,
            bytes: &[1, 0x80],
            written: &[0, 0xc0],
            address: 0x8000_4001,
            registers: &[(Gpr32::Ecx, 0x8000_4001)],
        },
        Case {
            name: "CL count also supplies a wrapping scaled index",
            code: &[0xd2, 0x44, 0x8b, 0xfc],
            operation: Operation::Rol,
            bits: 8,
            count: 1,
            input: 0x81,
            bytes: &[0x81],
            written: &[3],
            address: 0x4010,
            registers: &[(Gpr32::Ebx, 0x4010), (Gpr32::Ecx, 0x4000_0001)],
        },
        Case {
            name: "dword rotate crosses noncontiguous pages",
            code: &[0xc1, 0x0b, 31],
            operation: Operation::Ror,
            bits: 32,
            count: 31,
            input: 0x8000_0001,
            bytes: &[1, 0, 0, 0x80],
            written: &[3, 0, 0, 0],
            address: 0x4ffe,
            registers: &[(Gpr32::Ebx, 0x4ffe)],
        },
        Case {
            name: "large byte rotate preserves the width of the ring",
            code: &[0xc0, 0x0b, 255],
            operation: Operation::Ror,
            bits: 8,
            count: 255,
            input: 0x7f,
            bytes: &[0x7f],
            written: &[0xfe],
            address: 0x4011,
            registers: &[(Gpr32::Ebx, 0x4011)],
        },
        Case {
            name: "masked-zero byte count retains memory and the complete flags record",
            code: &[0xc0, 0x0b, 32],
            operation: Operation::Ror,
            bits: 8,
            count: 32,
            input: 0x81,
            bytes: &[0x81],
            written: &[0x81],
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "zero CL count checks both word pages without changing state",
            code: &[0x66, 0xd3, 0x0b],
            operation: Operation::Ror,
            bits: 16,
            count: 0,
            input: 0x8001,
            bytes: &[1, 0x80],
            written: &[1, 0x80],
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff), (Gpr32::Ecx, 0x8877_6600)],
        },
    ] {
        let mut image = image(case.code);
        for &(register, value) in case.registers {
            image.cpu.registers[register] = value;
        }
        image.map(case.address >> 12, 0x8000, true);
        let offset = case.address & 0xfff;
        let physical = 0x8000 + offset;
        let first_len = case.bytes.len().min((0x1000 - offset) as usize);
        image.data(physical - 1, &[0x5a]);
        image.data(physical, &case.bytes[..first_len]);
        let mut writes = vec![(physical, &case.written[..first_len])];
        if first_len < case.bytes.len() {
            image.map((case.address >> 12) + 1, 0xa000, true);
            image.data(0xa000, &case.bytes[first_len..]);
            image.data(0xa000 + (case.bytes.len() - first_len) as u32, &[0x5a]);
            writes.push((0xa000, &case.written[first_len..]));
        } else if offset + (first_len as u32) < 0x1000 {
            image.data(physical + first_len as u32, &[0x5a]);
        }
        let mut cpu = image.cpu;
        expected(
            case.operation,
            case.bits,
            case.input,
            case.count,
            PRIOR_FLAGS,
        )
        .apply_flags(&mut cpu);
        cpu.eip += case.code.len() as u32;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            case.name,
            case.code,
            1,
            &image,
            &[Step {
                cpu,
                ram: &writes,
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}

#[test]
fn full_write_access_is_required_before_any_rotate_or_flag_effect() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        count: u8,
        address: u32,
        first_writable: Option<bool>,
        second_writable: Option<bool>,
        fault_address: u32,
        error: u16,
    }
    for case in [
        Case {
            name: "zero byte count still requires a present page",
            code: &[0xc0, 0x03, 0],
            count: 0,
            address: 0x4020,
            first_writable: None,
            second_writable: None,
            fault_address: 0x4020,
            error: 2,
        },
        Case {
            name: "masked-zero dword count still requires write permission",
            code: &[0xc1, 0x0b, 32],
            count: 32,
            address: 0x4020,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4020,
            error: 3,
        },
        Case {
            name: "a full byte turn still requires write permission",
            code: &[0xc0, 0x03, 8],
            count: 8,
            address: 0x4020,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4020,
            error: 3,
        },
        Case {
            name: "zero CL count checks a missing second word page",
            code: &[0x66, 0xd3, 0x0b],
            count: 0,
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0x5000,
            error: 2,
        },
        Case {
            name: "nonzero rotate checks a read-only second dword page",
            code: &[0xd1, 0x03],
            count: 1,
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Case {
            name: "word range cannot wrap",
            code: &[0x66, 0xc1, 0x0b, 1],
            count: 1,
            address: 0xffff_ffff,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_ffff,
            error: 2,
        },
        Case {
            name: "dword range cannot wrap",
            code: &[0xd3, 0x0b],
            count: 255,
            address: 0xffff_fffd,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_fffd,
            error: 2,
        },
    ] {
        let mut image = image(case.code);
        image.cpu.registers.ebx = case.address;
        image.cpu.registers.ecx = 0x8877_6600 | u32::from(case.count);
        if let Some(writable) = case.first_writable {
            image.map(case.address >> 12, 0x8000, writable);
        }
        if let Some(writable) = case.second_writable {
            image.map(5, 0xa000, writable);
        }
        image.data(0x8020, &[0x81, 0x80, 0xff, 0xff]);
        image.data(0x8ffc, &[0x5a, 0x78, 0x56, 0x34]);
        image.data(0xa000, &[0x12, 0x5a]);
        both(
            TestModule::interpreter(),
            case.name,
            case.code,
            1,
            &image,
            &[Step {
                cpu: image.cpu,
                ram: &[],
                exit: Exit::PageFault {
                    address: case.fault_address,
                    error: case.error,
                },
            }],
        );
    }
}
