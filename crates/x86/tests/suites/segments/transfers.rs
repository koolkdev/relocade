use super::{code, data};
use crate::support::cases::{
    test_cases,
    FlagExpectation::Preserved,
    Flags, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    Gpr32::{Eax, Ebx, Ecx, Esp},
    Segment,
};

fn jumps() -> Vec<Case> {
    let mut cases = Vec::new();
    for bytes in [
        &[0xeb, 0x1e][..],
        &[0xe9, 0x1b, 0, 0, 0],
        &[0x66, 0xe9, 0x1c, 0],
        &[0xff, 0xe0],
        &[0xff, 0x23],
    ] {
        for (limit, valid) in [(0x1020, true), (0x101f, false)] {
            let case = Case::preserving_flags(
                format!("near jump {bytes:02x?} to CS limit {limit:x}"),
                bytes,
            )
            .segmented_only()
            .segment(Segment::Cs, code(0x8000, limit))
            .initial_register(Eax, 0x1020)
            .initial_register(Ebx, 0x4000)
            .memory(0x4000, &[0x20, 0x10, 0, 0], ReadOnly);
            cases.push(if valid {
                case.dispatch(0x1020)
            } else {
                case.general_protection(0)
            });
        }
    }
    cases.push(
        Case::preserving_flags(
            "word relative target truncates before CS validation",
            &[0x66, 0xe9, 0x10, 0],
        )
        .at(0xfff0)
        .segmented_only()
        .segment(Segment::Cs, code(0x8000, 0xfff3))
        .dispatch(4),
    );
    cases.push(
        Case::preserving_flags(
            "word indirect target ignores the register's upper word",
            &[0x66, 0xff, 0xe0],
        )
        .segmented_only()
        .segment(Segment::Cs, code(0x8000, 0x1002))
        .initial_register(Eax, 0xabcd_0010)
        .dispatch(0x10),
    );
    cases
}

test_cases!(
    near_jumps_check_cs_offsets_after_operand_size_truncation,
    jumps()
);

fn conditionals() -> Vec<Case> {
    let mut cases = Vec::new();
    for (bytes, uses_count, requires_zf) in [
        (&[0x74, 0x1e][..], false, Some(true)),
        (&[0x0f, 0x84, 0x1a, 0, 0, 0], false, Some(true)),
        (&[0x66, 0x75, 0x1d], false, Some(false)),
        (&[0xe3, 0x1e], true, None),
        (&[0xe2, 0x1e], true, None),
        (&[0xe1, 0x1e], true, Some(true)),
        (&[0xe0, 0x1e], true, Some(false)),
    ] {
        let loop_instruction = matches!(bytes[0], 0xe0..=0xe2);
        for taken in [false, true] {
            for target_fits in [false, true] {
                let zf =
                    requires_zf.is_some_and(|required| if taken { required } else { !required });
                let count = if bytes[0] == 0xe3 {
                    if taken {
                        0
                    } else {
                        1
                    }
                } else if loop_instruction && requires_zf.is_none() {
                    if taken {
                        2
                    } else {
                        1
                    }
                } else {
                    2
                };
                let flags = Flags {
                    zf,
                    ..Flags::all(false)
                };
                // The smaller limit puts fallthrough itself one byte beyond CS.
                let limit = if target_fits {
                    0x1020
                } else {
                    0x1000 + bytes.len() as u32 - 1
                };
                let mut case = Case::new(
                    format!("conditional {bytes:02x?}, taken {taken}, target fits {target_fits}"),
                    bytes,
                    flags,
                    Flags::all(Preserved),
                )
                .preserve_flag_record()
                .segmented_only()
                .segment(Segment::Cs, code(0x8000, limit));
                if uses_count {
                    case = case.register(
                        Ecx,
                        count,
                        if loop_instruction && (!taken || target_fits) {
                            count.wrapping_sub(1)
                        } else {
                            count
                        },
                    );
                }
                cases.push(if !taken {
                    case
                } else if target_fits {
                    case.dispatch(0x1020)
                } else {
                    case.general_protection(0)
                });
            }
        }
    }
    cases
}

test_cases!(
    conditional_targets_fault_only_when_taken_and_preserve_loop_count,
    conditionals()
);

fn calls() -> Vec<Case> {
    let mut cases = Vec::new();
    for bytes in [&[0xe8, 0x1b, 0, 0, 0][..], &[0xff, 0xd0], &[0xff, 0x13]] {
        for invalid_stack_segment in [false, true] {
            let mut case = Case::preserving_flags(
                format!(
                    "CALL target faults before stack, {bytes:02x?}, bad SS {invalid_stack_segment}"
                ),
                bytes,
            )
            .segmented_only()
            .segment(Segment::Cs, code(0x8000, 0x101f))
            .initial_register(Eax, 0x1020)
            .initial_register(Ebx, 0x4000)
            .initial_register(Esp, 0x8008)
            .memory(0x4000, &[0x20, 0x10, 0, 0], ReadOnly)
            .general_protection(0);
            if invalid_stack_segment {
                case = case.segment(Segment::Ss, data(0, 0x8000));
            }
            cases.push(case);
        }
        let saved = if bytes[0] == 0xe8 { 0x1005u32 } else { 0x1002 };
        cases.push(
            Case::preserving_flags(
                format!("CALL at CS limit pushes the return offset {bytes:02x?}"),
                bytes,
            )
            .segmented_only()
            .segment(Segment::Cs, code(0x8000, 0x1020))
            .initial_register(Eax, 0x1020)
            .initial_register(Ebx, 0x4000)
            .register(Esp, 0x8004, 0x8000)
            .memory(0x4000, &[0x20, 0x10, 0, 0], ReadOnly)
            .memory(0x8000, &[0xff; 4], ReadWrite)
            .expect_memory(0x8000, &saved.to_le_bytes())
            .dispatch(0x1020),
        );
    }
    cases.push(
        Case::preserving_flags(
            "CALL reads its memory target before considering stack faults",
            &[0xff, 0x13],
        )
        .segmented_only()
        .segment(Segment::Cs, code(0x8000, 0x1001))
        .initial_register(Ebx, 0x4000)
        .initial_register(Esp, 0x8004)
        .segment(Segment::Ss, data(0, 0))
        .fault(0x4000, 0),
    );
    cases.push(
        Case::preserving_flags(
            "word CALL checks the truncated target and pushes a word offset",
            &[0x66, 0xe8, 0x10, 0],
        )
        .at(0xfff0)
        .segmented_only()
        .segment(Segment::Cs, code(0x8000, 0xfff3))
        .register(Esp, 0x8002, 0x8000)
        .memory(0x8000, &[0xff; 4], ReadWrite)
        .expect_memory(0x8000, &[0xf4, 0xff, 0xff, 0xff])
        .dispatch(4),
    );
    cases
}

test_cases!(
    call_target_checks_precede_stack_changes_and_push_offsets,
    calls()
);

fn returns() -> Vec<Case> {
    let mut cases = Vec::new();
    for (bytes, width, discard) in [
        (&[0xc3][..], 4, 0),
        (&[0xc2, 0x34, 0x12], 4, 0x1234),
        (&[0x66, 0xc3], 2, 0),
        (&[0x66, 0xc2, 0x34, 0x12], 2, 0x1234),
    ] {
        for target in [0x1020u32, 0x1021] {
            let case =
                Case::preserving_flags(format!("RET {bytes:02x?}, target {target:x}"), bytes)
                    .segmented_only()
                    .segment(Segment::Cs, code(0x8000, 0x1020))
                    .segment(Segment::Ss, data(0x4000, 0x20 + width - 1))
                    .register(
                        Esp,
                        0x20,
                        if target == 0x1020 {
                            0x20 + width + discard
                        } else {
                            0x20
                        },
                    )
                    .memory(0x4020, &target.to_le_bytes(), ReadOnly);
            cases.push(if target == 0x1020 {
                case.dispatch(target)
            } else {
                case.general_protection(0)
            });
        }
        cases.push(
            Case::preserving_flags(
                format!("RET reads its stack before validating target {bytes:02x?}"),
                bytes,
            )
            .segmented_only()
            .segment(Segment::Cs, code(0x8000, 0x1020))
            .initial_register(Esp, 0x4000)
            .fault(0x4000, 0),
        );
        cases.push(
            Case::preserving_flags(
                format!("RET rejects its stack range before reading a target {bytes:02x?}"),
                bytes,
            )
            .segmented_only()
            .segment(Segment::Cs, code(0x8000, 0x1020))
            .segment(Segment::Ss, data(0, 0x4000))
            .initial_register(Esp, 0x4000)
            .stack_fault(0),
        );
    }
    cases
}

test_cases!(
    ret_validates_target_before_committing_stack_pointer_and_discard,
    returns()
);
