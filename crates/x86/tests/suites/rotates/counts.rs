use wasm86_x86::{Gpr32, StatusFlags};

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, Operation, OPERATIONS, PRIOR_FLAGS};

#[test]
fn masked_counts_and_full_turns_use_each_operand_width() {
    for bits in [8, 16, 32] {
        let (initial, upper) = match bits {
            8 => (0x81, 0x4433_2200),
            16 => (0x8001, 0x4433_0000),
            32 => (0x8000_0001, 0),
            _ => unreachable!(),
        };
        let counts = [0, 1, 7, 8, 9, 15, 16, 17, 24, 31, 32, 33, 255];
        for operation in OPERATIONS {
            for &count in &counts {
                for from_cl in [false, true] {
                    let mut code = Vec::new();
                    if bits == 16 {
                        code.push(0x66);
                    }
                    code.extend_from_slice(&[
                        if from_cl { 0xd2 } else { 0xc0 } + u8::from(bits != 8),
                        0xc0 | (operation.extension() << 3),
                    ]);
                    if !from_cl {
                        code.push(count);
                    }
                    let mut image = image(&code);
                    image.cpu.registers.eax = upper | initial;
                    image.cpu.registers.ecx = 0x8877_6600 | u32::from(count);
                    let result = expected(operation, bits, initial, count, PRIOR_FLAGS);
                    let mut cpu = image.cpu;
                    cpu.registers.eax = upper | result.value;
                    result.apply_flags(&mut cpu);
                    cpu.eip += code.len() as u32;
                    cpu.instruction_count = 0;
                    both(
                        TestModule::interpreter(),
                        &format!("{operation:?} {bits}-bit by {count}, CL source {from_cl}"),
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

#[test]
fn implicit_one_forms_update_only_carry_and_overflow() {
    for (operation, values, results, carry, overflow) in [
        (Operation::Rol, [0x81, 0x8001, 0x8000_0001], [3, 3, 3], 1, 1),
        (
            Operation::Ror,
            [0x81, 0x8001, 0x8000_0001],
            [0xc0, 0xc000, 0xc000_0000],
            1,
            0,
        ),
        (
            Operation::Rol,
            [0x40, 0x4000, 0x4000_0000],
            [0x80, 0x8000, 0x8000_0000],
            0,
            1,
        ),
        (Operation::Ror, [1, 1, 1], [0x80, 0x8000, 0x8000_0000], 1, 1),
    ] {
        for (width, bits) in [8, 16, 32].into_iter().enumerate() {
            let mut code = Vec::new();
            if bits == 16 {
                code.push(0x66);
            }
            code.extend_from_slice(&[
                0xd0 + u8::from(bits != 8),
                0xc0 | (operation.extension() << 3),
            ]);
            let upper = [0x4433_2200, 0x4433_0000, 0][width];
            let mut image = image(&code);
            image.cpu.registers.eax = upper | values[width];
            let mut cpu = image.cpu;
            cpu.registers.eax = upper | results[width];
            cpu.flags.kind = 0;
            cpu.flags.status = StatusFlags {
                cf: carry,
                of: overflow,
                ..PRIOR_FLAGS
            };
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!("implicit one {operation:?}, {bits}-bit"),
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
fn full_turns_change_flags_even_when_the_result_is_unchanged() {
    for (bits, counts) in [(8, &[8_u8, 16, 24][..]), (16, &[16_u8][..])] {
        for operation in OPERATIONS {
            for &count in counts {
                let mut code = if bits == 16 { vec![0x66] } else { vec![] };
                code.extend_from_slice(&[
                    0xc0 + u8::from(bits != 8),
                    0xc0 | (operation.extension() << 3),
                    count,
                ]);
                let mut image = image(&code);
                image.cpu.registers.eax = if operation == Operation::Rol {
                    1 << (bits - 1)
                } else {
                    1
                };
                let mut cpu = image.cpu;
                // The outgoing carry is zero, opposite to the incoming SUB carry.
                cpu.flags.kind = 0;
                cpu.flags.status = StatusFlags {
                    cf: 0,
                    of: 0,
                    ..PRIOR_FLAGS
                };
                cpu.eip += code.len() as u32;
                cpu.instruction_count = 0;
                both(
                    TestModule::interpreter(),
                    &format!("full-turn {operation:?} {bits} by {count}"),
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

#[test]
fn register_and_count_aliases_use_the_old_cl_and_preserve_other_bits() {
    struct Case {
        name: &'static str,
        code: &'static [u8],
        operation: Operation,
        bits: u32,
        register: Gpr32,
        before: u32,
        after: u32,
        operand: u32,
        count: u8,
    }
    for case in [
        Case {
            name: "ROL CL,CL takes the old count",
            code: &[0xd2, 0xc1],
            operation: Operation::Rol,
            bits: 8,
            register: Gpr32::Ecx,
            before: 0x8877_6603,
            after: 0x8877_6618,
            operand: 3,
            count: 3,
        },
        Case {
            name: "ROR CH,CL keeps the count byte",
            code: &[0xd2, 0xcd],
            operation: Operation::Ror,
            bits: 8,
            register: Gpr32::Ecx,
            before: 0x8877_8001,
            after: 0x8877_4001,
            operand: 0x80,
            count: 1,
        },
        Case {
            name: "ROR CX,CL reads the full old word",
            code: &[0x66, 0xd3, 0xc9],
            operation: Operation::Ror,
            bits: 16,
            register: Gpr32::Ecx,
            before: 0x8877_8001,
            after: 0x8877_c000,
            operand: 0x8001,
            count: 1,
        },
        Case {
            name: "ROL ECX,CL reads both old views",
            code: &[0xd3, 0xc1],
            operation: Operation::Rol,
            bits: 32,
            register: Gpr32::Ecx,
            before: 0x8000_0001,
            after: 3,
            operand: 0x8000_0001,
            count: 1,
        },
        Case {
            name: "AH changes without changing AL",
            code: &[0xd2, 0xcc],
            operation: Operation::Ror,
            bits: 8,
            register: Gpr32::Eax,
            before: 0x4433_8111,
            after: 0x4433_c011,
            operand: 0x81,
            count: 1,
        },
        Case {
            name: "byte form ignores operand-size override",
            code: &[0x66, 0xd0, 0xc7],
            operation: Operation::Rol,
            bits: 8,
            register: Gpr32::Ebx,
            before: 0x10ff_81dd,
            after: 0x10ff_03dd,
            operand: 0x81,
            count: 1,
        },
        Case {
            name: "word SP retains the upper half",
            code: &[0x66, 0xc1, 0xcc, 4],
            operation: Operation::Ror,
            bits: 16,
            register: Gpr32::Esp,
            before: 0x8765_8010,
            after: 0x8765_0801,
            operand: 0x8010,
            count: 4,
        },
    ] {
        let mut image = image(case.code);
        image.cpu.registers.ecx = 0x8877_6600 | u32::from(case.count);
        image.cpu.registers[case.register] = case.before;
        let mut cpu = image.cpu;
        cpu.registers[case.register] = case.after;
        expected(
            case.operation,
            case.bits,
            case.operand,
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
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}
