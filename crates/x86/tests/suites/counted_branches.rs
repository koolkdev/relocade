#[path = "counted_branches/decoding.rs"]
mod decoding;
#[path = "counted_branches/flags.rs"]
mod flags;
#[path = "counted_branches/sequences.rs"]
mod sequences;

use crate::support::cases::{
    test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case, Permissions::ReadOnly,
};
use wasm86_x86::Gpr32::Ecx;

const FORMS: [(u8, &str); 4] = [
    (0xe3, "JECXZ"),
    (0xe2, "LOOP"),
    (0xe1, "LOOPE"),
    (0xe0, "LOOPNE"),
];

fn input_flags(zero: bool) -> Flags<bool> {
    Flags {
        zf: zero,
        ..Flags::all(true)
    }
}

#[rustfmt::skip]
fn count_cases() -> Vec<Case> {
    // Each row states ECX before/after and taken outcomes for ZF clear/set.
    let outcomes = [
        [
            (0, 0, true, true),
            (1, 1, false, false),
            (2, 2, false, false),
            (0xffff_ffff, 0xffff_ffff, false, false),
            (0x0001_0000, 0x0001_0000, false, false),
            (0x0001_0001, 0x0001_0001, false, false),
        ],
        [
            (0, 0xffff_ffff, true, true),
            (1, 0, false, false),
            (2, 1, true, true),
            (0xffff_ffff, 0xffff_fffe, true, true),
            (0x0001_0000, 0x0000_ffff, true, true),
            (0x0001_0001, 0x0001_0000, true, true),
        ],
        [
            (0, 0xffff_ffff, false, true),
            (1, 0, false, false),
            (2, 1, false, true),
            (0xffff_ffff, 0xffff_fffe, false, true),
            (0x0001_0000, 0x0000_ffff, false, true),
            (0x0001_0001, 0x0001_0000, false, true),
        ],
        [
            (0, 0xffff_ffff, true, false),
            (1, 0, false, false),
            (2, 1, true, false),
            (0xffff_ffff, 0xffff_fffe, true, false),
            (0x0001_0000, 0x0000_ffff, true, false),
            (0x0001_0001, 0x0001_0000, true, false),
        ],
    ];
    let mut cases = Vec::new();
    for ((opcode, name), counts) in FORMS.into_iter().zip(outcomes) {
        for (before, after, clear_taken, set_taken) in counts {
            for (zero, taken) in [(false, clear_taken), (true, set_taken)] {
                for (prefix, target, fallthrough) in [
                    (&[][..], 0x1081, 0x1002),
                    (&[0x66][..], 0x1082, 0x1003),
                ] {
                    let code = [prefix, &[opcode, 0x7f]].concat();
                    cases.push(Case::new(format!("{name} {code:02x?}, ECX={before:08x}, ZF={zero}"),
                        &code, input_flags(zero), Flags::all(Preserved))
                        .register(Ecx, before, after).preserve_flag_record()
                        .dispatch(if taken { target } else { fallthrough }));
                }
            }
        }
    }
    cases
}

#[rustfmt::skip]
fn target_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (origin, prefix, displacement, target, fallthrough) in [
        (0x1000, &[][..], 0, 0x1002, 0x1002),
        (0x1000, &[], 0x7f, 0x1081, 0x1002),
        (0x1000, &[], 0x80, 0x0f82, 0x1002),
        (0x1000, &[], 0xfe, 0x1000, 0x1002),
        (0, &[], 0x80, 0xffff_ff82, 2),
        (0xffff_fffe, &[], 0x7f, 0x7f, 0),
        (0xffff_ffff, &[], 0x80, 0xffff_ff81, 1),
        (0x1234_1000, &[0x66], 0x7f, 0x1082, 0x1234_1003),
        (0x1234_1000, &[0x66], 0x80, 0x0f83, 0x1234_1003),
        (0x1234_fffe, &[0x66], 0, 1, 0x1235_0001),
        (0xffff_fffe, &[0x66], 0xff, 0, 1),
        (0x1234_1000, &[0x66, 0x66], 0xfc, 0x1000, 0x1234_1004),
    ] {
        for (opcode, count, after, zero, taken) in [
            (0xe3, 0, 0, false, true), (0xe3, 1, 1, false, false),
            (0xe2, 2, 1, false, true), (0xe2, 1, 0, false, false),
            (0xe1, 2, 1, true, true), (0xe1, 2, 1, false, false),
            (0xe0, 2, 1, false, true), (0xe0, 2, 1, true, false),
        ] {
            let code = [prefix, &[opcode, displacement]].concat();
            let mut case = Case::new(format!("counted branch {code:02x?} at {origin:08x}, taken={taken}"),
                &code, input_flags(zero), Flags::all(Preserved))
                .at(origin).register(Ecx, count, after).preserve_flag_record()
                .dispatch(if taken { target } else { fallthrough })
                .map_page(origin >> 12, 0x3000, ReadOnly);
            if (origin & 0xfff) as usize + code.len() > 0x1000 {
                case = case.map_page(origin.wrapping_add(code.len() as u32 - 1) >> 12, 0x5000, ReadOnly);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(full_ecx_counts_and_zero_conditions, count_cases());
test_cases!(signed_targets_and_operand_size, target_cases());
