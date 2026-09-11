use crate::support::{
    cases::test_cases,
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{bit_at_a_time_model, image, OPERATIONS};

#[test]
fn both_count_forms_match_the_independent_bit_at_a_time_model() {
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
                        let shifted =
                            bit_at_a_time_model(operation, bits, destination, source, count);
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

#[path = "counts/cases.rs"]
mod cases;

test_cases!(literal_results, cases::literal_results());
test_cases!(register_aliases, cases::alias_cases());
