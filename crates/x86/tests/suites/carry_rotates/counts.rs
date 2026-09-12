use wasm86_x86::FlagBytes;

use crate::support::{
    cases::test_cases,
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{bit_at_a_time_model, image, prior_flags, Operation, OPERATIONS};

#[path = "counts/cases.rs"]
mod cases;

#[test]
fn counted_rotations_match_the_independent_bit_at_a_time_model() {
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
                        let result =
                            bit_at_a_time_model(operation, bits, input, count, prior_flags(carry));
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
fn a_full_ring_plus_one_bit_still_uses_zero_for_undefined_overflow() {
    for (bits, input, upper, counts) in [
        (8, 0x81, 0x4433_2200, &[10, 19, 28, 42][..]),
        (16, 0x8001, 0x4433_0000, &[18, 50][..]),
    ] {
        for operation in OPERATIONS {
            for carry in [0, 1] {
                // These literal results equal a one-bit rotation. Their masked
                // counts exceed one, so they must not use one-bit OF semantics.
                let value = match (operation, carry, bits) {
                    (Operation::Rcl, 0, _) => 2,
                    (Operation::Rcl, 1, _) => 3,
                    (Operation::Rcr, 0, 8) => 0x40,
                    (Operation::Rcr, 1, 8) => 0xc0,
                    (Operation::Rcr, 0, 16) => 0x4000,
                    (Operation::Rcr, 1, 16) => 0xc000,
                    _ => unreachable!(),
                };
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
                        let mut cpu = image.cpu;
                        cpu.registers.eax = upper | value;
                        cpu.flags.bytes = FlagBytes {
                            cf: 1,
                            pf: 0,
                            af: 1,
                            zf: 1,
                            sf: 0,
                            of: 0,
                            ..cpu.flags.bytes
                        };
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

test_cases!(implicit_one, cases::implicit_one_cases());
test_cases!(count_aliases, cases::alias_cases());
