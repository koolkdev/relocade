use wasm86_x86::{Gpr32, StatusFlags};

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, Operation, OPERATIONS};

#[test]
fn both_count_forms_mask_before_shifting_at_each_operand_width() {
    for (bits, counts) in [
        (
            16,
            &[
                0, 1, 2, 7, 8, 15, 16, 17, 18, 31, 32, 33, 40, 47, 48, 49, 255,
            ][..],
        ),
        (32, &[0, 1, 2, 15, 16, 17, 31, 32, 33, 63, 255][..]),
    ] {
        let mask = u32::MAX >> (32 - bits);
        let upper = if bits == 16 { 0x4433_0000 } else { 0 };
        for (destination, source) in [
            (0, 0),
            (mask, mask),
            ((1 << (bits - 1)) | 1, 0x5aa5),
            (0x1234_5678 & mask, 0x89ab_cdef & mask),
        ] {
            for operation in OPERATIONS {
                for &count in counts {
                    for from_cl in [false, true] {
                        let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                        code.extend_from_slice(&[0x0f, operation.opcode(from_cl), 0xd0]);
                        if !from_cl {
                            code.push(count);
                        }
                        let mut image = image(&code);
                        image.cpu.registers.eax = upper | destination;
                        image.cpu.registers.edx = if bits == 16 {
                            0xccbb_0000 | source
                        } else {
                            source
                        };
                        image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
                        let shifted = expected(operation, bits, destination, source, count);
                        let mut cpu = image.cpu;
                        cpu.registers.eax = upper | shifted.value;
                        shifted.apply_flags(&mut cpu);
                        cpu.eip += code.len() as u32;
                        cpu.instruction_count = 0;
                        both(
                            TestModule::interpreter(),
                            &format!("{operation:?} {bits}-bit {destination:x},{source:x} by {count}, CL {from_cl}"),
                            &code,
                            1,
                            &image,
                            &[Step { cpu, ram: &[], exit: Exit::Dispatch(cpu.eip) }],
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn literal_one_and_word_width_results_check_carry_overflow_and_source_order() {
    struct Case {
        operation: Operation,
        bits: u32,
        destination: u32,
        source: u32,
        count: u8,
        result: u32,
        flags: StatusFlags,
    }
    for case in [
        Case {
            operation: Operation::Shld,
            bits: 32,
            destination: 0x8000_0000,
            source: 0,
            count: 1,
            result: 0,
            flags: StatusFlags {
                cf: 1,
                pf: 1,
                af: 0,
                zf: 1,
                sf: 0,
                of: 1,
            },
        },
        Case {
            operation: Operation::Shrd,
            bits: 32,
            destination: 0x8000_0000,
            source: 1,
            count: 1,
            result: 0xc000_0000,
            flags: StatusFlags {
                cf: 0,
                pf: 1,
                af: 0,
                zf: 0,
                sf: 1,
                of: 0,
            },
        },
        Case {
            operation: Operation::Shrd,
            bits: 32,
            destination: 0x8000_0000,
            source: 0,
            count: 1,
            result: 0x4000_0000,
            flags: StatusFlags {
                cf: 0,
                pf: 1,
                af: 0,
                zf: 0,
                sf: 0,
                of: 1,
            },
        },
        Case {
            operation: Operation::Shld,
            bits: 16,
            destination: 0x4000,
            source: 0,
            count: 1,
            result: 0x8000,
            flags: StatusFlags {
                cf: 0,
                pf: 1,
                af: 0,
                zf: 0,
                sf: 1,
                of: 1,
            },
        },
        Case {
            operation: Operation::Shld,
            bits: 16,
            destination: 0x8001,
            source: 0x1234,
            count: 16,
            result: 0x1234,
            flags: StatusFlags {
                cf: 1,
                pf: 0,
                af: 0,
                zf: 0,
                sf: 0,
                of: 0,
            },
        },
        Case {
            operation: Operation::Shrd,
            bits: 16,
            destination: 0x8000,
            source: 0xabcd,
            count: 16,
            result: 0xabcd,
            flags: StatusFlags {
                cf: 1,
                pf: 0,
                af: 0,
                zf: 0,
                sf: 1,
                of: 0,
            },
        },
    ] {
        let mut code = if case.bits == 16 { vec![0x66] } else { vec![] };
        code.extend_from_slice(&[0x0f, case.operation.opcode(false), 0xd0, case.count]);
        let mut image = image(&code);
        let upper = if case.bits == 16 { 0x4433_0000 } else { 0 };
        image.cpu.registers.eax = upper | case.destination;
        image.cpu.registers.edx = case.source;
        let mut cpu = image.cpu;
        cpu.registers.eax = upper | case.result;
        cpu.flags.kind = 0;
        cpu.flags.status = case.flags;
        cpu.eip += code.len() as u32;
        cpu.instruction_count = 0;
        both(
            TestModule::interpreter(),
            &format!(
                "literal {:?} {}-bit by {}",
                case.operation, case.bits, case.count
            ),
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
fn zero_counts_retain_every_byte_of_incoming_lazy_and_concrete_flags() {
    for kind in [0, 1, 2, 3, 5, 6, 7, 9, 10, 11] {
        for operation in OPERATIONS {
            for bits in [16, 32] {
                for (count, from_cl) in [(0, false), (32, false), (0, true), (32, true)] {
                    let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                    code.extend_from_slice(&[0x0f, operation.opcode(from_cl), 0xd0]);
                    if !from_cl {
                        code.push(count);
                    }
                    let mut image = image(&code);
                    image.cpu.flags.kind = kind;
                    image.cpu.flags.left = 0x1234_80ff;
                    image.cpu.flags.right = 0x8765_0101;
                    image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
                    let mut cpu = image.cpu;
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = 0;
                    both(
                        TestModule::interpreter(),
                        &format!("{operation:?} zero retains raw flag kind {kind}, {bits}-bit, CL {from_cl}"),
                        &code, 1, &image,
                        &[Step { cpu, ram: &[], exit: Exit::Dispatch(cpu.eip) }],
                    );
                }
            }
        }
    }
}

#[test]
fn destination_source_and_cl_aliases_capture_the_old_values() {
    struct Case {
        name: &'static str,
        destination: Gpr32,
        source: Gpr32,
        modrm: u8,
        from_cl: bool,
        count: u8,
    }
    for case in [
        Case {
            name: "same destination and source",
            destination: Gpr32::Eax,
            source: Gpr32::Eax,
            modrm: 0xc0,
            from_cl: false,
            count: 16,
        },
        Case {
            name: "destination contains CL",
            destination: Gpr32::Ecx,
            source: Gpr32::Edx,
            modrm: 0xd1,
            from_cl: true,
            count: 3,
        },
        Case {
            name: "source contains CL",
            destination: Gpr32::Eax,
            source: Gpr32::Ecx,
            modrm: 0xc8,
            from_cl: true,
            count: 3,
        },
        Case {
            name: "destination and source both contain CL",
            destination: Gpr32::Ecx,
            source: Gpr32::Ecx,
            modrm: 0xc9,
            from_cl: true,
            count: 3,
        },
        Case {
            name: "SP destination is a register without SIB",
            destination: Gpr32::Esp,
            source: Gpr32::Edi,
            modrm: 0xfc,
            from_cl: false,
            count: 16,
        },
    ] {
        for operation in OPERATIONS {
            for bits in [16, 32] {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[0x0f, operation.opcode(case.from_cl), case.modrm]);
                if !case.from_cl {
                    code.push(case.count);
                }
                let mut image = image(&code);
                image.cpu.registers[case.destination] = 0x4433_8001;
                image.cpu.registers[case.source] = 0xccbb_a55a;
                image.cpu.registers.ecx = 0x8877_8000 | u32::from(case.count);
                let destination = image.cpu.registers[case.destination];
                let source = image.cpu.registers[case.source];
                let shifted = expected(operation, bits, destination, source, case.count);
                let mask = u32::MAX >> (32 - bits);
                let mut cpu = image.cpu;
                cpu.registers[case.destination] = (destination & !mask) | shifted.value;
                shifted.apply_flags(&mut cpu);
                cpu.eip += code.len() as u32;
                cpu.instruction_count = 0;
                both(
                    TestModule::interpreter(),
                    &format!("{operation:?} {bits}-bit {}", case.name),
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
    }
}
