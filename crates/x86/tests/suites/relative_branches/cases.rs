use crate::support::cases::{
    test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case, Permissions::ReadOnly,
};

#[rustfmt::skip]
fn jump_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (start, code, target) in [
        (0x1000, &[0xeb, 0][..], 0x1002),
        (0x1000, &[0xeb, 0x7f], 0x1081),
        (0x1000, &[0xeb, 0x80], 0x0f82),
        (0x1000, &[0xeb, 0xfe], 0x1000),
        (0, &[0xeb, 0x80], 0xffff_ff82),
        (0xffff_fffe, &[0xeb, 0x7f], 0x7f),
        (0x1000, &[0xe9, 0, 0, 0, 0], 0x1005),
        (0x1000, &[0xe9, 0xff, 0xff, 0xff, 0x7f], 0x8000_1004),
        (0x1000, &[0xe9, 0, 0, 0, 0x80], 0x8000_1005),
        (0, &[0xe9, 0xfa, 0xff, 0xff, 0xff], 0xffff_ffff),
        (0xffff_fffc, &[0xe9, 0, 0, 0, 0], 1),
        (0x1234_fffe, &[0x66, 0xeb, 0], 1),
        (0x1234_1000, &[0x66, 0xeb, 0x80], 0x0f83),
        (0x1234_1000, &[0x66, 0xe9, 0, 0], 0x1004),
        (0x1234_1000, &[0x66, 0xe9, 0xff, 0x7f], 0x9003),
        (0x1234_1000, &[0x66, 0xe9, 0, 0x80], 0x9004),
        (0x1234_fffe, &[0x66, 0xe9, 0xfd, 0xff], 0xffff),
        (0x1234_1000, &[0x66, 0x66, 0xe9, 0xfb, 0xff], 0x1000),
    ] {
        let mut case = Case::preserving_flags(format!("JMP {code:02x?} at {start:08x}"), code).at(start).dispatch(target)
            .map_page(start >> 12, 0x3000, ReadOnly);
        if (start & 0xfff) as usize + code.len() > 0x1000 {
            case = case.map_page(start.wrapping_add(code.len() as u32 - 1) >> 12, 0x5000, ReadOnly);
        }
        cases.push(case);
    }
    cases
}

#[rustfmt::skip]
fn word_condition_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (start, code, taken_target, fallthrough) in [
        (0x1234_fffe, &[0x66, 0x74, 0][..], 1, 0x1235_0001),
        (0x1234_1000, &[0x66, 0x74, 0x80], 0x0f83, 0x1234_1003),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0, 0][..],
            0x1005,
            0x1234_1005,
        ),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0xff, 0x7f],
            0x9004,
            0x1234_1005,
        ),
        (
            0x1234_1000,
            &[0x66, 0x0f, 0x84, 0, 0x80],
            0x9005,
            0x1234_1005,
        ),
        (
            0x1234_fffe,
            &[0x66, 0x0f, 0x84, 0xfc, 0xff],
            0xffff,
            0x1235_0003,
        ),
        (0xffff_fffd, &[0x66, 0x0f, 0x84, 0xfd, 0xff], 0xffff, 2),
    ] {
        for (zero, target) in [(false, fallthrough), (true, taken_target)] {
            let flags = Flags { cf: true, pf: true, af: true, zf: zero, sf: true, of: true };
            let mut case = Case::new(format!("word JE {code:02x?} at {start:08x}, ZF={zero}"), code, flags, Flags::all(Preserved))
                .at(start).dispatch(target).preserve_flag_record().map_page(start >> 12, 0x3000, ReadOnly);
            if (start & 0xfff) as usize + code.len() > 0x1000 {
                case = case.map_page(start.wrapping_add(code.len() as u32 - 1) >> 12, 0x5000, ReadOnly);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(signed_relative_jumps, jump_cases());
test_cases!(conditional_word_truncation, word_condition_cases());

#[rustfmt::skip]
fn completed_encoding_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, taken_when_zero_clear) in [
        (&[0xeb, 1][..], true), (&[0x66, 0xeb, 1], true), (&[0xe9, 1, 0, 0, 0], true), (&[0x66, 0xe9, 1, 0], true),
        (&[0x74, 1], false), (&[0x66, 0x74, 1], false), (&[0x0f, 0x84, 1, 0, 0, 0], false), (&[0x66, 0x0f, 0x84, 1, 0], false),
    ] {
        for zero in [false, true] {
            let target = if zero || taken_when_zero_clear { 0x2001 } else { 0x2000 };
            cases.push(Case::new(format!("successor absent after {code:02x?}, ZF={zero}"), code,
                Flags { cf: true, pf: true, af: true, zf: zero, sf: true, of: true }, Flags::all(Preserved))
                .at(0x2000 - code.len() as u32).dispatch(target).preserve_flag_record());
        }
    }
    for (prefixes, suffix) in [(13, &[0xeb, 1][..]), (12, &[0xe9, 1, 0]), (13, &[0x74, 1]), (11, &[0x0f, 0x84, 1, 0])] {
        let code = [vec![0x66; prefixes], suffix.to_vec()].concat();
        cases.push(Case::new(format!("fifteen-byte branch {suffix:02x?}"), &code, Flags::all(true), Flags::all(Preserved))
            .at(0x1ff1).dispatch(0x2001).preserve_flag_record());
    }
    cases
}
test_cases!(complete_branch_encodings, completed_encoding_cases());
