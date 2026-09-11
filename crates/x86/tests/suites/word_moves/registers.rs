use crate::support::cases::{test_cases, InstructionCase as Case};

use super::REGISTERS;

#[rustfmt::skip]
fn immediate_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (register, immediate, output) in [
        (0, 0x8000_u16, 0x4433_8000), (1, 0, 0x8877_0000),
        (2, 0xffff, 0xccbb_ffff), (3, 0x6688, 0x10ff_6688),
        (4, 0x1234, 0x7654_1234), (5, 0xc7a1, 0xfedc_c7a1),
        (6, 0x7fff, 0x0123_7fff), (7, 0xb88a, 0x89ab_b88a),
    ] {
        let [low, high] = immediate.to_le_bytes();
        let (name, destination, input) = REGISTERS[register as usize];
        for code in [vec![0x66, 0xb8 + register, low, high], vec![0x66, 0xc7, 0xc0 + register, low, high]] {
            cases.push(Case::preserving_flags(format!("MOV {name},imm16 via {:02x}", code[1]), &code)
                .register(destination, input, output));
        }
    }
    cases
}

#[rustfmt::skip]
fn register_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    // Every encoding occupies both operand roles, including SP–DI and self moves.
    for (source, destination, output) in [
        (0, 4, 0x7654_2211), (1, 5, 0xfedc_6655), (2, 6, 0x0123_aa99),
        (3, 7, 0x89ab_eedd), (4, 0, 0x4433_3210), (5, 1, 0x8877_ba98),
        (6, 2, 0xccbb_4567), (7, 3, 0x10ff_cdef), (0, 0, 0x4433_2211), (7, 7, 0x89ab_cdef),
    ] {
        let (source_name, source_register, source_input) = REGISTERS[source as usize];
        let (destination_name, destination_register, destination_input) = REGISTERS[destination as usize];
        for code in [
            [0x66, 0x89, 0xc0 | (source << 3) | destination],
            [0x66, 0x8b, 0xc0 | (destination << 3) | source],
        ] {
            let mut case = Case::preserving_flags(format!("MOV {destination_name},{source_name} via {:02x}", code[1]), &code)
                .register(destination_register, destination_input, output);
            if source_register != destination_register {
                case = case.initial_register(source_register, source_input);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(word_immediate_registers, immediate_cases());
test_cases!(word_register_encodings, register_cases());
