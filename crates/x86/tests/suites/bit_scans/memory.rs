use wasm86_x86::Gpr32;

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, retire, OPERATIONS};

#[test]
fn scans_read_zero_and_nonzero_full_operands_from_readonly_and_scattered_pages() {
    for (bits, address) in [
        (16, 0x4001_u32),
        (16, 0x4ffe),
        (16, 0x4fff),
        (32, 0x4001),
        (32, 0x4ffc),
        (32, 0x4ffd),
        (32, 0x4ffe),
        (32, 0x4fff),
        (16, 0xffff_fffe),
        (32, 0xffff_fffc),
    ] {
        for operation in OPERATIONS {
            for source in [0_u32, 1, (1 << (bits - 1)) | 8] {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, operation.opcode(), 0x03]);
                let mut image = image(&code);
                image.cpu.registers.ebx = address;
                let bytes = source.to_le_bytes();
                let length = (bits / 8) as usize;
                let offset = address & 0xfff;
                let physical = 0x8000 + offset;
                let first_length = length.min((0x1000 - offset) as usize);
                image.map(address >> 12, 0x8000, false);
                image.data(physical - 1, &[0x5a]);
                image.data(physical, &bytes[..first_length]);
                if first_length < length {
                    image.map((address >> 12) + 1, 0xa000, false);
                    image.data(0xa000, &bytes[first_length..length]);
                    image.data(0xa000 + (length - first_length) as u32, &[0x5a]);
                } else if offset + (first_length as u32) < 0x1000 {
                    image.data(physical + first_length as u32, &[0x5a]);
                }
                let mut cpu = image.cpu;
                expected(operation, bits, source, cpu.registers.eax).apply(&mut cpu, Gpr32::Eax);
                let step = retire(&mut cpu, code.len() as u32);
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?} {bits}-bit source {source:x} at {address:08x}"),
                    &code,
                    1,
                    &image,
                    &[step],
                );
            }
        }
    }
}

#[test]
fn a_scan_destination_can_supply_its_old_full_base_or_scaled_index() {
    struct Case {
        name: &'static str,
        bits: u32,
        destination: Gpr32,
        registers: &'static [(Gpr32, u32)],
        address_bytes: &'static [u8],
        address: u32,
    }
    for case in [
        Case {
            name: "EAX is the base",
            bits: 32,
            destination: Gpr32::Eax,
            registers: &[(Gpr32::Eax, 0x4000)],
            address_bytes: &[0x00],
            address: 0x4000,
        },
        Case {
            name: "AX retains the high word of the full EAX base",
            bits: 16,
            destination: Gpr32::Eax,
            registers: &[(Gpr32::Eax, 0x8000_4020)],
            address_bytes: &[0x00],
            address: 0x8000_4020,
        },
        Case {
            name: "ECX is a wrapping scaled index",
            bits: 32,
            destination: Gpr32::Ecx,
            registers: &[(Gpr32::Ebx, 0x4010), (Gpr32::Ecx, 0x4000_0001)],
            address_bytes: &[0x4c, 0x8b, 0xfc],
            address: 0x4010,
        },
        Case {
            name: "ESP is the SIB base",
            bits: 32,
            destination: Gpr32::Esp,
            registers: &[(Gpr32::Esp, 0x4000)],
            address_bytes: &[0x24, 0x24],
            address: 0x4000,
        },
    ] {
        for operation in OPERATIONS {
            for source in [0_u32, 0x8000_8008] {
                let mut code = if case.bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, operation.opcode()]);
                code.extend_from_slice(case.address_bytes);
                let mut image = image(&code);
                for &(register, value) in case.registers {
                    image.cpu.registers[register] = value;
                }
                let physical = 0x8000 + (case.address & 0xfff);
                let length = (case.bits / 8) as usize;
                image.map(case.address >> 12, 0x8000, false);
                image.data(physical - 1, &[0x5a]);
                image.data(physical, &source.to_le_bytes()[..length]);
                image.data(physical + length as u32, &[0x5a]);
                let mut cpu = image.cpu;
                expected(
                    operation,
                    case.bits,
                    source,
                    cpu.registers[case.destination],
                )
                .apply(&mut cpu, case.destination);
                let step = retire(&mut cpu, code.len() as u32);
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?}: {}, source {source:x}", case.name),
                    &code,
                    1,
                    &image,
                    &[step],
                );
            }
        }
    }
}

#[test]
fn source_faults_preserve_the_destination_and_flags_before_any_scan_result() {
    for (bits, address, first_present, fault) in [
        (16, 0x4000_u32, false, 0x4000),
        (32, 0x4000, false, 0x4000),
        (16, 0x4fff, true, 0x5000),
        (32, 0x4ffd, true, 0x5000),
        (32, 0x4ffe, true, 0x5000),
        (32, 0x4fff, true, 0x5000),
        (16, u32::MAX, true, u32::MAX),
        (32, 0xffff_fffe, true, 0xffff_fffe),
    ] {
        for operation in OPERATIONS {
            for first_byte in [0, 1] {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, operation.opcode(), 0x03]);
                let mut image = image(&code);
                image.cpu.registers.ebx = address;
                if first_present {
                    image.map(address >> 12, 0x8000, false);
                }
                // Even a first-byte set bit cannot let BSF skip the rest of
                // the source's permission checks. Zero cannot suppress them.
                image.data(0x8000 + (address & 0xfff), &[first_byte]);
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?} {bits}-bit full source at {address:x}, first byte {first_byte}"),
                    &code,
                    1,
                    &image,
                    &[Step {
                        cpu: image.cpu,
                        ram: &[],
                        exit: Exit::PageFault {
                            address: fault,
                            error: 0,
                        },
                    }],
                );
            }
        }
    }
}
