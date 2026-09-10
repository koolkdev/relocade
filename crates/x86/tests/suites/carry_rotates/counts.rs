use wasm86_x86::{Gpr32, StatusFlags};

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, prior_flags, Operation, OPERATIONS};

#[test]
fn masked_counts_include_complete_carry_rings_and_their_neighbors() {
    for (bits, input, upper, counts) in [
        (
            8,
            0x81,
            0x4433_2200,
            &[
                0, 1, 2, 7, 8, 9, 10, 17, 18, 19, 26, 27, 28, 31, 32, 33, 40, 41, 42, 255,
            ][..],
        ),
        (
            16,
            0x8001,
            0x4433_0000,
            &[0, 1, 2, 15, 16, 17, 18, 31, 32, 33, 34, 49, 50, 255][..],
        ),
        (
            32,
            0x8000_0001,
            0,
            &[0, 1, 2, 15, 16, 17, 30, 31, 32, 33, 34, 255][..],
        ),
    ] {
        for operation in OPERATIONS {
            for carry in [0, 1] {
                for &count in counts {
                    for from_cl in [false, true] {
                        let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                        code.extend_from_slice(&[
                            if from_cl { 0xd2 } else { 0xc0 } + u8::from(bits != 8),
                            0xc0 | (operation.extension() << 3),
                        ]);
                        if !from_cl {
                            code.push(count);
                        }
                        let mut image = image(&code, carry);
                        image.cpu.registers.eax = upper | input;
                        image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
                        let result = expected(operation, bits, input, count, prior_flags(carry));
                        let mut cpu = image.cpu;
                        cpu.registers.eax = upper | result.value;
                        result.apply_flags(&mut cpu);
                        cpu.eip += code.len() as u32;
                        cpu.instruction_count = 0;
                        both(
                            TestModule::interpreter(),
                            &format!(
                                "{operation:?} {bits}-bit by {count}, CF {carry}, CL {from_cl}"
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
            }
        }
    }
}

#[test]
fn implicit_one_consumes_incoming_carry_at_each_operand_width() {
    for (operation, carry, inputs, outputs, cf, of) in [
        (
            Operation::Rcl,
            0,
            [0x81, 0x8001, 0x8000_0001],
            [2, 2, 2],
            1,
            1,
        ),
        (
            Operation::Rcl,
            1,
            [0x81, 0x8001, 0x8000_0001],
            [3, 3, 3],
            1,
            1,
        ),
        (
            Operation::Rcr,
            0,
            [0x81, 0x8001, 0x8000_0001],
            [0x40, 0x4000, 0x4000_0000],
            1,
            1,
        ),
        (
            Operation::Rcr,
            1,
            [0x81, 0x8001, 0x8000_0001],
            [0xc0, 0xc000, 0xc000_0000],
            1,
            0,
        ),
        (Operation::Rcl, 1, [0, 0, 0], [1, 1, 1], 0, 0),
        (
            Operation::Rcr,
            1,
            [0, 0, 0],
            [0x80, 0x8000, 0x8000_0000],
            0,
            1,
        ),
        (
            Operation::Rcl,
            0,
            [0xff, 0xffff, u32::MAX],
            [0xfe, 0xfffe, 0xffff_fffe],
            1,
            0,
        ),
        (
            Operation::Rcr,
            0,
            [0xff, 0xffff, u32::MAX],
            [0x7f, 0x7fff, 0x7fff_ffff],
            1,
            1,
        ),
    ] {
        for (index, bits) in [8, 16, 32].into_iter().enumerate() {
            let mut code = if bits == 16 { vec![0x66] } else { vec![] };
            code.extend_from_slice(&[
                0xd0 + u8::from(bits != 8),
                0xc0 | (operation.extension() << 3),
            ]);
            let upper = [0x4433_2200, 0x4433_0000, 0][index];
            let mut image = image(&code, carry);
            image.cpu.registers.eax = upper | inputs[index];
            let mut cpu = image.cpu;
            cpu.registers.eax = upper | outputs[index];
            cpu.flags.kind = 0;
            cpu.flags.status = StatusFlags {
                cf,
                of,
                ..prior_flags(carry)
            };
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!(
                    "implicit {operation:?} {bits}-bit input {:x}, CF {carry}",
                    inputs[index]
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
}

#[test]
fn count_and_destination_aliases_capture_old_register_values() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        operation: Operation,
        bits: u32,
        register: Gpr32,
        shift: u32,
        before: u32,
        count: u8,
    }
    for case in [
        Case {
            name: "RCL CL,CL",
            code: &[0xd2, 0xd1],
            operation: Operation::Rcl,
            bits: 8,
            register: Gpr32::Ecx,
            shift: 0,
            before: 0x8877_6603,
            count: 3,
        },
        Case {
            name: "RCR CH,CL",
            code: &[0xd2, 0xdd],
            operation: Operation::Rcr,
            bits: 8,
            register: Gpr32::Ecx,
            shift: 8,
            before: 0x8877_8001,
            count: 1,
        },
        Case {
            name: "RCL CX,CL",
            code: &[0x66, 0xd3, 0xd1],
            operation: Operation::Rcl,
            bits: 16,
            register: Gpr32::Ecx,
            shift: 0,
            before: 0x8877_8001,
            count: 1,
        },
        Case {
            name: "RCR ECX,CL",
            code: &[0xd3, 0xd9],
            operation: Operation::Rcr,
            bits: 32,
            register: Gpr32::Ecx,
            shift: 0,
            before: 0x8000_0001,
            count: 1,
        },
        Case {
            name: "RCL AH,CL",
            code: &[0xd2, 0xd4],
            operation: Operation::Rcl,
            bits: 8,
            register: Gpr32::Eax,
            shift: 8,
            before: 0x4433_8111,
            count: 1,
        },
        Case {
            name: "byte RCR ignores operand-size override",
            code: &[0x66, 0xd0, 0xdf],
            operation: Operation::Rcr,
            bits: 8,
            register: Gpr32::Ebx,
            shift: 8,
            before: 0x10ff_81dd,
            count: 1,
        },
        Case {
            name: "RCL SP,17 preserves its upper word",
            code: &[0x66, 0xc1, 0xd4, 17],
            operation: Operation::Rcl,
            bits: 16,
            register: Gpr32::Esp,
            shift: 0,
            before: 0x8765_8010,
            count: 17,
        },
    ] {
        for carry in [0, 1] {
            let mask = u32::MAX >> (32 - case.bits);
            let input = (case.before >> case.shift) & mask;
            let result = expected(
                case.operation,
                case.bits,
                input,
                case.count,
                prior_flags(carry),
            );
            let mut image = image(case.code, carry);
            image.cpu.registers.ecx = 0x8877_6600 | u32::from(case.count);
            image.cpu.registers[case.register] = case.before;
            let mut cpu = image.cpu;
            cpu.registers[case.register] =
                (case.before & !(mask << case.shift)) | (result.value << case.shift);
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
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip),
                }],
            );
        }
    }
}
