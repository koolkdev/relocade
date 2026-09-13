use super::{code16, stack16};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    Gpr32::{Eax, Esp},
    Segment, StoredSegment,
};

fn transfers() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, bytes) in [
        (&[][..], 4u32),
        (&[0x66][..], 2),
        (&[0x67][..], 4),
        (&[0x67, 0x66][..], 2),
    ] {
        let payload = &[0x78, 0x56, 0x34, 0x12][..bytes as usize];
        let push = [prefix, &[0x50]].concat();
        cases.push(
            Case::preserving_flags(format!("SS.B=0 PUSH {push:02x?}"), &push)
                .segmented_only()
                .segment(Segment::Ss, stack16(0xffff))
                .initial_register(Eax, 0x1234_5678)
                .register(Esp, 0xabcd_0000, 0xabcd_0000 | (0x10000 - bytes))
                .memory(0x10000 - bytes, &vec![0xff; bytes as usize], ReadWrite)
                .expect_memory(0x10000 - bytes, payload),
        );
        let pop = [prefix, &[0x58]].concat();
        cases.push(
            Case::preserving_flags(format!("SS.B=0 POP {pop:02x?}"), &pop)
                .segmented_only()
                .segment(Segment::Ss, stack16(0xffff))
                .register(
                    Eax,
                    0xaaaa_0000,
                    if bytes == 4 { 0x1234_5678 } else { 0xaaaa_5678 },
                )
                .register(Esp, 0xabcd_0000 | (0x10000 - bytes), 0xabcd_0000)
                .memory(0x10000 - bytes, payload, ReadOnly),
        );
    }
    cases.extend([
        code16(
            Case::preserving_flags("16-bit code pushes a word using a 32-bit stack", &[0x50])
                .initial_register(Eax, 0x1234_5678)
                .register(Esp, 0x14002, 0x14000)
                .memory(0x14000, &[0xff; 2], ReadWrite)
                .expect_memory(0x14000, &[0x78, 0x56]),
        ),
        code16(
            Case::preserving_flags(
                "16-bit code CALL saves a word return offset",
                &[0xe8, 0xfd, 0x0f],
            )
            .segment(Segment::Ss, stack16(0xffff))
            .register(Esp, 0xabcd_8002, 0xabcd_8000)
            .memory(0x8000, &[0xff; 2], ReadWrite)
            .expect_memory(0x8000, &[3, 0x10])
            .dispatch(0x2000),
        ),
        Case::preserving_flags("POP ESP replaces the increment on a 16-bit stack", &[0x5c])
            .segmented_only()
            .segment(Segment::Ss, stack16(0xffff))
            .register(Esp, 0xabcd_8000, 0x1234_5678)
            .memory(0x8000, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags(
            "POP SP preserves upper ESP on a 16-bit stack",
            &[0x66, 0x5c],
        )
        .segmented_only()
        .segment(Segment::Ss, stack16(0xffff))
        .register(Esp, 0xabcd_fffe, 0xabcd_5678)
        .memory(0xfffe, &[0x78, 0x56], ReadOnly),
        Case::preserving_flags(
            "RET discard wraps SP and preserves upper ESP",
            &[0x66, 0xc2, 0xff, 0xff],
        )
        .segmented_only()
        .segment(Segment::Ss, stack16(0xffff))
        .register(Esp, 0xabcd_fffe, 0xabcd_ffff)
        .memory(0xfffe, &[0, 0x20], ReadOnly)
        .dispatch(0x2000),
        Case::preserving_flags(
            "POP memory sees next full ESP with a 16-bit stack",
            &[0x8f, 0x04, 0x24],
        )
        .segmented_only()
        .segment(Segment::Ss, stack16(u32::MAX))
        .register(Esp, 0x18000, 0x18004)
        .memory(0x8000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
        .memory(0x18004, &[0xff; 4], ReadWrite)
        .expect_memory(0x18004, &[0x78, 0x56, 0x34, 0x12]),
    ]);
    cases
}

fn faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("stack access checks every byte after SP wraps", &[0x50])
            .segmented_only()
            .segment(Segment::Ss, stack16(0xffff))
            .initial_register(Esp, 0xabcd_0002)
            .memory(0xfffe, &[0xff; 4], ReadWrite)
            .stack_fault(0),
        Case::preserving_flags(
            "POP destination fault keeps entry SP and full ESP",
            &[0x8f, 0x04, 0x24],
        )
        .segmented_only()
        .segment(Segment::Ss, stack16(u32::MAX))
        .initial_register(Esp, 0x18000)
        .memory(0x8000, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
        .fault(0x18004, 2),
        Case::preserving_flags(
            "RET target fault keeps SP before pop and discard",
            &[0x66, 0xc2, 0xff, 0xff],
        )
        .segmented_only()
        .segment(Segment::Ss, stack16(0xffff))
        .segment(
            Segment::Cs,
            StoredSegment {
                limit: 0x1fff,
                ..StoredSegment::flat_code32(0)
            },
        )
        .initial_register(Esp, 0xabcd_fffe)
        .memory(0xfffe, &[0, 0x20], ReadOnly)
        .general_protection(0),
        Case::preserving_flags(
            "CALL target fault precedes 16-bit stack write",
            &[0xe8, 0xfb, 0x0f, 0, 0],
        )
        .segmented_only()
        .segment(Segment::Ss, stack16(0xffff))
        .segment(
            Segment::Cs,
            StoredSegment {
                limit: 0x1fff,
                ..StoredSegment::flat_code32(0)
            },
        )
        .initial_register(Esp, 0xabcd_0000)
        .memory(0xfffc, &[0xff; 4], ReadWrite)
        .general_protection(0),
    ]
}

test_cases!(
    ss_b_is_independent_of_code_operand_and_address_sizes,
    transfers()
);
test_cases!(stack_and_target_faults_preserve_entry_state, faults());
