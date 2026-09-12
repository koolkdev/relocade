use super::{carry_result, flag_records, logical_flags};
use crate::support::cases::{test_cases, InstructionCase as Case};

fn logical_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for bits in 0_u8..64 {
        let initial = logical_flags(bits);
        for (name, opcode, result) in [
            ("CLC", 0xf8, false),
            ("STC", 0xf9, true),
            ("CMC", 0xf5, !initial.cf),
        ] {
            for prefixes in [0, 1, 2] {
                let mut code = vec![0x66; prefixes];
                code.push(opcode);
                cases.push(Case::new(
                    format!("{name}, status {bits:02x}, {prefixes} operand prefixes"),
                    &code,
                    initial,
                    carry_result(result),
                ));
            }
        }
    }
    cases
}

fn stored_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (record_name, record, initial) in flag_records() {
        for (name, opcode, result) in [
            ("CLC", 0xf8, false),
            ("STC", 0xf9, true),
            ("CMC", 0xf5, !initial.cf),
        ] {
            cases.push(
                Case::new(
                    format!("{name}, {record_name}"),
                    &[opcode],
                    initial,
                    carry_result(result),
                )
                .stored_flags(record),
            );
        }
    }
    cases
}

test_cases!(only_carry_changes_for_every_logical_input, logical_cases());
test_cases!(
    stored_recipes_preserve_the_other_logical_flags,
    stored_cases()
);
