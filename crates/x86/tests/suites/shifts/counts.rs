use crate::support::{
    cases::test_cases,
    machine::{both, Exit, Step},
    step::TestModule,
};

use super::{bit_at_a_time_model, image, OPERATIONS};

#[test]
fn immediate_and_cl_counts_match_the_independent_bit_at_a_time_model() {
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
                    let result = bit_at_a_time_model(operation, bits, initial, count);
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

#[path = "counts/cases.rs"]
mod cases;

test_cases!(implicit_one, cases::implicit_one_cases());
test_cases!(count_aliases, cases::alias_cases());
test_cases!(zero_count_records, cases::zero_count_records());
