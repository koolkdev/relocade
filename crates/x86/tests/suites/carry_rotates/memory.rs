use wasm86_x86::Gpr32;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, prior_flags, Operation, OPERATIONS};

#[test]
fn memory_carry_rotates_use_exact_widths_and_old_address_registers() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        operation: Operation,
        bits: u32,
        count: u8,
        input: u32,
        address: u32,
        registers: &'static [(Gpr32, u32)],
    }
    for case in [
        Case {
            name: "implicit byte carry rotate uses the last mapped byte",
            code: &[0xd0, 0x13],
            operation: Operation::Rcl,
            bits: 8,
            count: 1,
            input: 0x81,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "CL count shares the old full ECX address base",
            code: &[0x66, 0xd3, 0x19],
            operation: Operation::Rcr,
            bits: 16,
            count: 1,
            input: 0x8001,
            address: 0x8000_4001,
            registers: &[(Gpr32::Ecx, 0x8000_4001)],
        },
        Case {
            name: "CL count supplies an old wrapping scaled index",
            code: &[0xd2, 0x54, 0x8b, 0xfc],
            operation: Operation::Rcl,
            bits: 8,
            count: 1,
            input: 0x81,
            address: 0x4010,
            registers: &[(Gpr32::Ebx, 0x4010), (Gpr32::Ecx, 0x4000_0001)],
        },
        Case {
            name: "dword carry rotate spans noncontiguous pages",
            code: &[0xc1, 0x1b, 31],
            operation: Operation::Rcr,
            bits: 32,
            count: 31,
            input: 0x8000_0001,
            address: 0x4ffe,
            registers: &[(Gpr32::Ebx, 0x4ffe)],
        },
        Case {
            name: "large byte carry rotate retains its nine-bit ring",
            code: &[0xc0, 0x1b, 255],
            operation: Operation::Rcr,
            bits: 8,
            count: 255,
            input: 0x7f,
            address: 0x4011,
            registers: &[(Gpr32::Ebx, 0x4011)],
        },
        Case {
            name: "masked-zero memory count preserves raw flags",
            code: &[0xc0, 0x1b, 32],
            operation: Operation::Rcr,
            bits: 8,
            count: 32,
            input: 0x81,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "zero CL count checks both word pages",
            code: &[0x66, 0xd3, 0x13],
            operation: Operation::Rcl,
            bits: 16,
            count: 0,
            input: 0x8001,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "complete byte carry ring preserves the operand and carry",
            code: &[0xc0, 0x13, 9],
            operation: Operation::Rcl,
            bits: 8,
            count: 9,
            input: 0x81,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "complete word carry ring checks both mapped pages",
            code: &[0x66, 0xd3, 0x1b],
            operation: Operation::Rcr,
            bits: 16,
            count: 17,
            input: 0x8001,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
    ] {
        for carry in [0, 1] {
            let mut image = image(case.code, carry);
            image.cpu.registers.ecx = 0x8877_6600 | u32::from(case.count);
            for &(register, value) in case.registers {
                image.cpu.registers[register] = value;
            }
            let result = expected(
                case.operation,
                case.bits,
                case.input,
                case.count,
                prior_flags(carry),
            );
            let bytes = case.input.to_le_bytes();
            let written = result.value.to_le_bytes();
            let len = (case.bits / 8) as usize;
            image.map(case.address >> 12, 0x8000, true);
            let offset = case.address & 0xfff;
            let physical = 0x8000 + offset;
            let first_len = len.min((0x1000 - offset) as usize);
            image.data(physical - 1, &[0x5a]);
            image.data(physical, &bytes[..first_len]);
            let mut writes = vec![(physical, &written[..first_len])];
            if first_len < len {
                image.map((case.address >> 12) + 1, 0xa000, true);
                image.data(0xa000, &bytes[first_len..len]);
                image.data(0xa000 + (len - first_len) as u32, &[0x5a]);
                writes.push((0xa000, &written[first_len..len]));
            } else if offset + (first_len as u32) < 0x1000 {
                image.data(physical + first_len as u32, &[0x5a]);
            }
            let mut cpu = image.cpu;
            result.apply_flags(&mut cpu);
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
}

#[test]
fn full_write_access_precedes_any_carry_rotate_or_flag_effect() {
    struct Case {
        name: &'static str,
        bits: u32,
        count: u8,
        from_cl: bool,
        address: u32,
        first_writable: Option<bool>,
        second_writable: Option<bool>,
        fault_address: u32,
        error: u16,
    }
    for case in [
        Case {
            name: "zero byte count needs a present page",
            bits: 8,
            count: 0,
            from_cl: false,
            address: 0x4020,
            first_writable: None,
            second_writable: None,
            fault_address: 0x4020,
            error: 2,
        },
        Case {
            name: "masked-zero dword count needs write permission",
            bits: 32,
            count: 32,
            from_cl: false,
            address: 0x4020,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4020,
            error: 3,
        },
        Case {
            name: "complete byte carry ring needs write permission",
            bits: 8,
            count: 9,
            from_cl: false,
            address: 0x4020,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4020,
            error: 3,
        },
        Case {
            name: "zero CL count checks the missing second word page",
            bits: 16,
            count: 0,
            from_cl: true,
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0x5000,
            error: 2,
        },
        Case {
            name: "complete word carry ring checks a read-only second page",
            bits: 16,
            count: 17,
            from_cl: true,
            address: 0x4fff,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Case {
            name: "nonzero dword count checks a read-only second page",
            bits: 32,
            count: 1,
            from_cl: false,
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Case {
            name: "the first word page fails before the second page",
            bits: 16,
            count: 17,
            from_cl: false,
            address: 0x4fff,
            first_writable: Some(false),
            second_writable: None,
            fault_address: 0x4fff,
            error: 3,
        },
        Case {
            name: "word operand range cannot wrap",
            bits: 16,
            count: 17,
            from_cl: false,
            address: 0xffff_ffff,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_ffff,
            error: 2,
        },
        Case {
            name: "dword operand range cannot wrap",
            bits: 32,
            count: 255,
            from_cl: true,
            address: 0xffff_fffd,
            first_writable: Some(true),
            second_writable: None,
            fault_address: 0xffff_fffd,
            error: 2,
        },
    ] {
        for operation in OPERATIONS {
            let mut code = if case.bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[
                if case.from_cl { 0xd2 } else { 0xc0 } + u8::from(case.bits != 8),
                0x03 | (operation.extension() << 3),
            ]);
            if !case.from_cl {
                code.push(case.count);
            }
            let mut image = image(&code, 1);
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
                &code,
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
}
