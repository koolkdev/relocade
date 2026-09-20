use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

#[path = "bounds/decoding.rs"]
mod decoding;
#[path = "bounds/memory.rs"]
mod memory;
#[path = "bounds/progress.rs"]
mod progress;

fn pair(word: bool, lower: i32, upper: i32) -> Vec<u8> {
    let width = if word { 2 } else { 4 };
    [&lower.to_le_bytes()[..width], &upper.to_le_bytes()[..width]].concat()
}

fn code16() -> StoredSegment {
    StoredSegment {
        attributes: SegmentAttributes::from_bits(0x07),
        ..StoredSegment::flat_code32(0x1b)
    }
}

fn segment(base: u32, limit: u32, attributes: u16) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        selector: 0x23,
        attributes: SegmentAttributes::from_bits(attributes),
    }
}

fn absolute_code(word: bool, default16: bool, register: u8, address: u32) -> Vec<u8> {
    let mut code = Vec::new();
    if word != default16 {
        code.push(0x66);
    }
    code.extend([0x62, (register << 3) | if default16 { 6 } else { 5 }]);
    code.extend_from_slice(&address.to_le_bytes()[..if default16 { 2 } else { 4 }]);
    code
}

fn absolute_case(name: impl Into<String>, word: bool, default16: bool, register: u8) -> Case {
    let case = Case::preserving_flags(name, &absolute_code(word, default16, register, 0x4000));
    if default16 {
        case.segmented_only().segment(Segment::Cs, code16())
    } else {
        case
    }
}

fn signed_ranges() -> Vec<Case> {
    let mut cases = Vec::new();
    for word in [false, true] {
        let (min, max) = if word {
            (-32768, 32767)
        } else {
            (i32::MIN, i32::MAX)
        };
        // The success column is authored independently of the implementation.
        for (index, lower, upper, success) in [
            (-6, -5, 7, false),
            (-5, -5, 7, true),
            (0, -5, 7, true),
            (7, -5, 7, true),
            (8, -5, 7, false),
            (11, -5, 7, false),
            (-1, -1, -1, true),
            (0, -1, -1, false),
            (-2, -1, -1, false),
            (min, min, max, true),
            (max, min, max, true),
            (min, min + 1, max, false),
            (max, min, max - 1, false),
            (min, max, min, false),
            (0, 7, -5, false),
            (7, 7, -5, false),
        ] {
            for default16 in [false, true] {
                let value = if word {
                    0xa5a5_0000 | (index as u32 & 0xffff)
                } else {
                    index as u32
                };
                let mut case = absolute_case(
                    format!("BOUND word={word} CS.D16={default16} {lower} <= {index} <= {upper}"),
                    word,
                    default16,
                    0,
                )
                .initial_register(Eax, value)
                .memory(0x4000, &pair(word, lower, upper), ReadOnly);
                if !success {
                    case = case.bound_range_exceeded();
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn index_registers() -> Vec<Case> {
    let mut cases = Vec::new();
    for (encoding, register) in [Eax, Ecx, Edx, Ebx, Esp, Ebp, Esi, Edi]
        .into_iter()
        .enumerate()
    {
        for word in [false, true] {
            for default16 in [false, true] {
                for (value, success) in [(-1i32, true), (1, false)] {
                    let value = if word {
                        0x1234_0000 | (value as u32 & 0xffff)
                    } else {
                        value as u32
                    };
                    let mut case = absolute_case(
                        format!(
                            "BOUND {register:?} word={word} CS.D16={default16} value={value:x}"
                        ),
                        word,
                        default16,
                        encoding as u8,
                    )
                    .initial_register(register, value)
                    .memory(0x4000, &pair(word, -2, 0), ReadOnly);
                    if !success {
                        case = case.bound_range_exceeded();
                    }
                    cases.push(case);
                }
            }
        }
    }
    cases
}

test_cases!(signed_inclusive_ranges, signed_ranges());
test_cases!(all_index_registers_preserve_state, index_registers());
