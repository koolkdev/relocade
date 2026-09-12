use super::{flag_records, logical_flags};
use crate::support::cases::{
    test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case,
};
use wasm86_x86::CpuState;

fn direction_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for bits in 0..64 {
        for (name, opcode, result) in [("CLD", 0xfc, false), ("STD", 0xfd, true)] {
            cases.push(
                Case::new(
                    format!("{name}, status {bits:02x}"),
                    &[opcode],
                    logical_flags(bits),
                    Flags::all(Preserved),
                )
                .preserve_flag_record()
                .expect_direction_flag(result),
            );
        }
    }
    for (record_name, mut record, flags) in flag_records() {
        for direction in [0, 1, 0x80, 0xff] {
            record.bytes.df = direction;
            for (name, opcode, result) in [("CLD", 0xfc, false), ("STD", 0xfd, true)] {
                for prefixes in [0, 1, 2] {
                    let mut code = vec![0x66; prefixes];
                    code.push(opcode);
                    cases.push(
                        Case::new(
                            format!("{name}, {record_name}, DF byte {direction:02x}, {prefixes} prefixes"),
                            &code,
                            flags,
                            Flags::all(Preserved),
                        )
                        .stored_flags(record)
                        .preserve_flag_record()
                        .expect_direction_flag(result),
                    );
                }
            }
        }
    }
    for fill in [0, 0x5a, 0xff] {
        for (opcode, result) in [(0xfc, false), (0xfd, true)] {
            cases.push(
                Case::preserving_flags(
                    format!("{opcode:02x} with opaque {fill:02x} record"),
                    &[opcode],
                )
                .stored_flags(CpuState::filled(fill).flags)
                .expect_direction_flag(result),
            );
        }
    }
    cases
}

test_cases!(only_the_direction_byte_changes, direction_cases());
