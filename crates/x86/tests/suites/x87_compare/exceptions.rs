//! Invalid and denormal operands control condition-code publication and pops.

use super::*;

fn nan_policy(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (instruction, pops, accepts_quiet_nan) in [
        ([0xd8, 0xd1], 0, false),
        ([0xd8, 0xd9], 1, false),
        ([0xde, 0xd9], 2, false),
        ([0xdd, 0xe1], 0, true),
        ([0xdd, 0xe9], 1, true),
        ([0xda, 0xe9], 2, true),
    ] {
        for (value, invalid_for_unordered) in [
            (QNAN, false),
            (SNAN, true),
            ((1, 0x3fff), true),
            ((0, 0x7fff), true),
        ] {
            for control in [0x037f, 0x037e] {
                let invalid = !accepts_quiet_nan || invalid_for_unordered;
                let suppressed = invalid && control == 0x037e;
                check_case(
                    &mut checks,
                    "NaN policy and unsupported encodings",
                    instruction,
                    Case {
                        left: ONE,
                        right: value,
                        control,
                        flags: if suppressed {
                            0x4101 | PENDING
                        } else {
                            UNORDERED | u16::from(invalid)
                        },
                        pops: if suppressed { 0 } else { pops },
                        empty: 0,
                    },
                );
            }
        }
    }
    for (left, right, flags) in [
        (QNAN, ONE, UNORDERED),
        (SNAN, ONE, UNORDERED | 1),
        (QNAN, SNAN, UNORDERED | 1),
        (SNAN, QNAN, UNORDERED | 1),
        (QNAN, (1, 0x7fff), UNORDERED | 1),
    ] {
        check_case(
            &mut checks,
            "both operands contribute invalid evidence",
            [0xda, 0xe9],
            Case {
                left,
                right,
                control: 0x037f,
                flags,
                pops: 2,
                empty: 0,
            },
        );
    }
}

fn denormals(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (left, right, relation) in [
        ((1, 0), (0, 0), 0),
        ((0, 0), (1, 0), LESS),
        ((1, 0x8000), (0, 0x8000), LESS),
        ((LEADING - 1, 0), (LEADING, 1), LESS),
        ((LEADING, 0), (LEADING, 1), EQUAL),
        ((LEADING + 1, 0x8000), (LEADING, 0x8001), LESS),
        ((1, 0), (2, 0), LESS),
    ] {
        for control in [0x037f, 0x037d] {
            check_case(
                &mut checks,
                "denormal and pseudo-denormal ordering",
                [0xde, 0xd9],
                Case {
                    left,
                    right,
                    control,
                    flags: if control == 0x037d {
                        0x4102 | PENDING
                    } else {
                        relation | 2
                    },
                    pops: if control == 0x037d { 0 } else { 2 },
                    empty: 0,
                },
            );
        }
    }
    for (instruction, left, right, flags) in [
        ([0xda, 0xe9], QNAN, (1, 0), UNORDERED),
        ([0xda, 0xe9], (1, 0), QNAN, UNORDERED),
        ([0xde, 0xd9], QNAN, (1, 0), UNORDERED | 1),
        ([0xda, 0xe9], SNAN, (1, 0), UNORDERED | 1),
        ([0xda, 0xe9], (1, 0x3fff), (1, 0), UNORDERED | 1),
    ] {
        check_case(
            &mut checks,
            "unordered response suppresses the denormal exception",
            instruction,
            Case {
                left,
                right,
                control: 0x037d,
                flags,
                pops: 2,
                empty: 0,
            },
        );
    }
}

fn empty_operands(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for empty in [1, 2, 3] {
        for control in [0x037f, 0x037e, 0x037d] {
            check_case(
                &mut checks,
                "stack underflow overrides operand exceptions",
                [0xda, 0xe9],
                Case {
                    left: (1, 0),
                    right: QNAN,
                    control,
                    flags: if control == 0x037e {
                        0x4141 | PENDING
                    } else {
                        UNORDERED | 0x41
                    },
                    pops: if control == 0x037e { 0 } else { 2 },
                    empty,
                },
            );
        }
    }
    for (left, empty, flags) in [(QNAN, 0, 0x4101 | PENDING), (ONE, 1, 0x4141 | PENDING)] {
        check_case(
            &mut checks,
            "FTST suppresses an unmasked invalid result",
            [0xd9, 0xe4],
            Case {
                left,
                right: ONE,
                control: 0x037e,
                flags,
                pops: 0,
                empty,
            },
        );
    }
}

fn quiet_nan_with_unmasked_denormal(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    check_case(
        &mut checks,
        "quiet NaN suppresses denormal evidence with both exceptions unmasked",
        [0xda, 0xe9],
        Case {
            left: QNAN,
            right: (1, 0),
            control: 0x037c,
            flags: UNORDERED,
            pops: 2,
            empty: 0,
        },
    );
}

test_frontends!(nan, nan_policy);
test_frontends!(denormal, denormals);
test_frontends!(stack, empty_operands);
test_frontends!(quiet_priority, quiet_nan_with_unmasked_denormal);
