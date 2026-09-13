use super::code16;
use crate::support::cases::{
    test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case,
};
use wasm86_x86::{Gpr32::Ecx, Segment, StoredSegment};

fn counters() -> Vec<Case> {
    let mut cases = Vec::new();
    for default16 in [false, true] {
        for (opcode, input, output, zero, taken) in [
            (0xe3, 0xabcd_0000, 0xabcd_0000, false, true),
            (0xe3, 0xabcd_0001, 0xabcd_0001, true, false),
            (0xe2, 0xabcd_0000, 0xabcd_ffff, false, true),
            (0xe2, 0xabcd_0001, 0xabcd_0000, true, false),
            (0xe1, 0xabcd_0002, 0xabcd_0001, true, true),
            (0xe1, 0xabcd_0002, 0xabcd_0001, false, false),
            (0xe0, 0xabcd_0002, 0xabcd_0001, false, true),
            (0xe0, 0xabcd_0002, 0xabcd_0001, true, false),
        ] {
            for wide_target in [false, true] {
                let mut code = if default16 { vec![] } else { vec![0x67] };
                if wide_target == default16 {
                    code.push(0x66);
                }
                code.extend([opcode, 0x10]);
                let fallthrough = 0x1fff0 + code.len() as u32;
                let target = if wide_target {
                    fallthrough + 0x10
                } else {
                    (fallthrough + 0x10) & 0xffff
                };
                let case = Case::new(format!("CX branch {opcode:02x}, CS.D16={default16}, wide target={wide_target}, count={input:x}"), &code,
                    Flags { zf: zero, ..Flags::all(false) }, Flags::all(Preserved))
                    .at(0x1fff0).register(Ecx, input, output).preserve_flag_record()
                    .dispatch(if taken { target } else { fallthrough });
                cases.push(if default16 { code16(case) } else { case });
            }
        }
    }
    cases.push(
        Case::preserving_flags(
            "taken LOOP fault preserves full ECX before CX decrement",
            &[0x67, 0xe2, 0x7f],
        )
        .segmented_only()
        .segment(
            Segment::Cs,
            StoredSegment {
                limit: 0x1002,
                ..StoredSegment::flat_code32(0)
            },
        )
        .initial_register(Ecx, 0xabcd_0002)
        .general_protection(0),
    );
    cases
}

test_cases!(
    loop_and_count_zero_use_address_size_independently_of_target_width,
    counters()
);
