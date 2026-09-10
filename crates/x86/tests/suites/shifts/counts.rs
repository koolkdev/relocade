use wasm86_x86::{Gpr32, StatusFlags};

use crate::support::{
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{expected, image, Operation, OPERATIONS};

#[test]
fn immediate_and_cl_counts_mask_to_five_bits_at_every_operand_width() {
    for bits in [8, 16, 32] {
        let (initial, upper) = match bits {
            8 => (0x81, 0x4433_2200),
            16 => (0x8001, 0x4433_0000),
            32 => (0x8000_0001, 0),
            _ => unreachable!(),
        };
        let mut counts = vec![0, 1, (bits - 1) as u8, bits as u8, 31, 32, 255];
        counts.sort_unstable();
        counts.dedup();
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
                    let result = expected(operation, bits, initial, count);
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
fn implicit_one_forms_publish_literal_status_flags() {
    struct Case {
        operation: Operation,
        values: [u32; 3],
        results: [u32; 3],
        carry: u8,
        parity: [u8; 3],
        sign: u8,
        overflow: u8,
    }
    for case in [
        Case {
            operation: Operation::Shl,
            values: [0x81, 0x8001, 0x8000_0001],
            results: [2, 2, 2],
            carry: 1,
            parity: [0, 0, 0],
            sign: 0,
            overflow: 1,
        },
        Case {
            operation: Operation::Shr,
            values: [0x81, 0x8001, 0x8000_0001],
            results: [0x40, 0x4000, 0x4000_0000],
            carry: 1,
            parity: [0, 1, 1],
            sign: 0,
            overflow: 1,
        },
        Case {
            operation: Operation::Sar,
            values: [0x81, 0x8001, 0x8000_0001],
            results: [0xc0, 0xc000, 0xc000_0000],
            carry: 1,
            parity: [1, 1, 1],
            sign: 1,
            overflow: 0,
        },
        Case {
            operation: Operation::Sar,
            values: [0x7f, 0x7fff, 0x7fff_ffff],
            results: [0x3f, 0x3fff, 0x3fff_ffff],
            carry: 1,
            parity: [1, 1, 1],
            sign: 0,
            overflow: 0,
        },
    ] {
        for (width, bits) in [8, 16, 32].into_iter().enumerate() {
            let mut code = Vec::new();
            if bits == 16 {
                code.push(0x66);
            }
            code.extend_from_slice(&[
                0xd0 + u8::from(bits != 8),
                0xc0 | (case.operation.extension() << 3),
            ]);
            let upper = [0x4433_2200, 0x4433_0000, 0][width];
            let mut image = image(&code);
            image.cpu.registers.eax = upper | case.values[width];
            let mut cpu = image.cpu;
            cpu.registers.eax = upper | case.results[width];
            cpu.flags.kind = 0;
            cpu.flags.status = StatusFlags {
                cf: case.carry,
                pf: case.parity[width],
                af: 0,
                zf: 0,
                sf: case.sign,
                of: case.overflow,
            };
            cpu.eip += code.len() as u32;
            cpu.instruction_count = 0;
            both(
                TestModule::interpreter(),
                &format!("implicit one {:?}, {bits}-bit", case.operation),
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
            name: "SHL CL,CL takes the old count",
            code: &[0xd2, 0xe1],
            operation: Operation::Shl,
            bits: 8,
            register: Gpr32::Ecx,
            before: 0x8877_6603,
            after: 0x8877_6618,
            operand: 3,
            count: 3,
        },
        Case {
            name: "SHR CH,CL keeps the count byte",
            code: &[0xd2, 0xed],
            operation: Operation::Shr,
            bits: 8,
            register: Gpr32::Ecx,
            before: 0x8877_8001,
            after: 0x8877_4001,
            operand: 0x80,
            count: 1,
        },
        Case {
            name: "SAR CX,CL reads the full old word",
            code: &[0x66, 0xd3, 0xf9],
            operation: Operation::Sar,
            bits: 16,
            register: Gpr32::Ecx,
            before: 0x8877_8001,
            after: 0x8877_c000,
            operand: 0x8001,
            count: 1,
        },
        Case {
            name: "SHL ECX,CL reads both old views",
            code: &[0xd3, 0xe1],
            operation: Operation::Shl,
            bits: 32,
            register: Gpr32::Ecx,
            before: 0x8000_0001,
            after: 2,
            operand: 0x8000_0001,
            count: 1,
        },
        Case {
            name: "AH changes without changing AL",
            code: &[0xd2, 0xfc],
            operation: Operation::Sar,
            bits: 8,
            register: Gpr32::Eax,
            before: 0x4433_8111,
            after: 0x4433_c011,
            operand: 0x81,
            count: 1,
        },
        Case {
            name: "byte form ignores operand-size override",
            code: &[0x66, 0xd0, 0xe7],
            operation: Operation::Shl,
            bits: 8,
            register: Gpr32::Ebx,
            before: 0x10ff_81dd,
            after: 0x10ff_02dd,
            operand: 0x81,
            count: 1,
        },
        Case {
            name: "word SP retains the upper half",
            code: &[0x66, 0xc1, 0xec, 4],
            operation: Operation::Shr,
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
        expected(case.operation, case.bits, case.operand, case.count).apply_flags(&mut cpu);
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
