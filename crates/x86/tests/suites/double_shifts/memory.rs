use wasm86_x86::Gpr32;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, OPERATIONS};

#[test]
fn memory_double_shifts_use_exact_widths_and_old_source_count_and_address_registers() {
    struct Case {
        name: &'static str,
        bits: u32,
        count: u8,
        from_cl: bool,
        modrm_and_address: &'static [u8],
        source: Gpr32,
        input: u32,
        address: u32,
        registers: &'static [(Gpr32, u32)],
    }
    for case in [
        Case {
            name: "word ends at the last mapped byte",
            bits: 16,
            count: 16,
            from_cl: false,
            modrm_and_address: &[0x13],
            source: Gpr32::Edx,
            input: 0x8001,
            address: 0x4ffe,
            registers: &[(Gpr32::Ebx, 0x4ffe)],
        },
        Case {
            name: "dword ends at the last mapped byte",
            bits: 32,
            count: 31,
            from_cl: true,
            modrm_and_address: &[0x13],
            source: Gpr32::Edx,
            input: 0x8000_0001,
            address: 0x4ffc,
            registers: &[(Gpr32::Ebx, 0x4ffc)],
        },
        Case {
            name: "word spans noncontiguous pages",
            bits: 16,
            count: 16,
            from_cl: true,
            modrm_and_address: &[0x13],
            source: Gpr32::Edx,
            input: 0x8001,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "dword spans noncontiguous pages",
            bits: 32,
            count: 1,
            from_cl: false,
            modrm_and_address: &[0x13],
            source: Gpr32::Edx,
            input: 0x8000_0001,
            address: 0x4ffe,
            registers: &[(Gpr32::Ebx, 0x4ffe)],
        },
        Case {
            name: "CL count shares the full ECX address base",
            bits: 16,
            count: 1,
            from_cl: true,
            modrm_and_address: &[0x11],
            source: Gpr32::Edx,
            input: 0x8001,
            address: 0x8000_4001,
            registers: &[(Gpr32::Ecx, 0x8000_4001)],
        },
        Case {
            name: "source supplies the address base",
            bits: 32,
            count: 16,
            from_cl: false,
            modrm_and_address: &[0x1b],
            source: Gpr32::Ebx,
            input: 0x1234_5678,
            address: 0x4010,
            registers: &[(Gpr32::Ebx, 0x4010)],
        },
        Case {
            name: "source count and address all share ECX",
            bits: 32,
            count: 1,
            from_cl: true,
            modrm_and_address: &[0x09],
            source: Gpr32::Ecx,
            input: 0x8000_0001,
            address: 0x8000_4001,
            registers: &[(Gpr32::Ecx, 0x8000_4001)],
        },
        Case {
            name: "CL count supplies a wrapping scaled index",
            bits: 32,
            count: 1,
            from_cl: true,
            modrm_and_address: &[0x74, 0x8b, 0xfc],
            source: Gpr32::Esi,
            input: 0x8000_0001,
            address: 0x4010,
            registers: &[(Gpr32::Ebx, 0x4010), (Gpr32::Ecx, 0x4000_0001)],
        },
        Case {
            name: "masked-zero dword preserves raw flags",
            bits: 32,
            count: 32,
            from_cl: false,
            modrm_and_address: &[0x13],
            source: Gpr32::Edx,
            input: 0x8000_0001,
            address: 0x4ffc,
            registers: &[(Gpr32::Ebx, 0x4ffc)],
        },
        Case {
            name: "zero CL count checks both word pages",
            bits: 16,
            count: 0,
            from_cl: true,
            modrm_and_address: &[0x13],
            source: Gpr32::Edx,
            input: 0x8001,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
        Case {
            name: "undefined word count follows the zero-result policy",
            bits: 16,
            count: 17,
            from_cl: false,
            modrm_and_address: &[0x13],
            source: Gpr32::Edx,
            input: 0x8001,
            address: 0x4fff,
            registers: &[(Gpr32::Ebx, 0x4fff)],
        },
    ] {
        for operation in OPERATIONS {
            let mut code = if case.bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[0x0f, operation.opcode(case.from_cl)]);
            code.extend_from_slice(case.modrm_and_address);
            if !case.from_cl {
                code.push(case.count);
            }
            let mut image = image(&code);
            image.cpu.registers.ecx = 0x8877_6600 | u32::from(case.count);
            image.cpu.registers.edx = 0xccbb_a55a;
            for &(register, value) in case.registers {
                image.cpu.registers[register] = value;
            }
            let shifted = expected(
                operation,
                case.bits,
                case.input,
                image.cpu.registers[case.source],
                case.count,
            );
            let bytes = case.input.to_le_bytes();
            let written = shifted.value.to_le_bytes();
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
            shifted.apply_flags(&mut cpu);
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                case.name,
                &code,
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
fn full_write_access_precedes_any_double_shift_or_flag_effect() {
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
            name: "zero word count needs a present page",
            bits: 16,
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
            name: "undefined word count still needs write permission",
            bits: 16,
            count: 17,
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
            name: "masked-zero CL count checks a read-only second page",
            bits: 32,
            count: 32,
            from_cl: true,
            address: 0x4ffe,
            first_writable: Some(true),
            second_writable: Some(false),
            fault_address: 0x5000,
            error: 3,
        },
        Case {
            name: "count at word width checks a read-only second page",
            bits: 16,
            count: 16,
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
            code.extend_from_slice(&[0x0f, operation.opcode(case.from_cl), 0x13]);
            if !case.from_cl {
                code.push(case.count);
            }
            let mut image = image(&code);
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
