use super::data;
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    Gpr32::{Eax, Ebx, Esp},
    Segment, StoredSegment,
};

fn stack_transfers() -> Vec<Case> {
    vec![
        Case::preserving_flags(
            "PUSH keeps its SS destination despite unusable FS override",
            &[0x64, 0x50],
        )
        .interpreter_only()
        .segment(Segment::Ss, data(0x8000, 0xff))
        .segment(Segment::Fs, StoredSegment::unusable(0x53))
        .initial_register(Eax, 0x1234_5678)
        .register(Esp, 0x24, 0x20)
        .memory(0x8020, &[0xff; 4], ReadWrite)
        .expect_memory(0x8020, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags(
            "POP keeps its SS source despite unusable FS override",
            &[0x64, 0x58],
        )
        .interpreter_only()
        .segment(Segment::Ss, data(0x8000, 0xff))
        .segment(Segment::Fs, StoredSegment::unusable(0x53))
        .register(Eax, 0, 0x1234_5678)
        .register(Esp, 0x20, 0x24)
        .memory(0x8020, &[0x78, 0x56, 0x34, 0x12], ReadOnly),
        Case::preserving_flags("PUSH memory reads FS then writes SS", &[0x64, 0xff, 0x33])
            .interpreter_only()
            .segment(Segment::Ss, data(0x8000, 0xff))
            .segment(Segment::Fs, data(0x4000, 0xff))
            .initial_register(Ebx, 0x20)
            .register(Esp, 0x24, 0x20)
            .memory(0x4020, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
            .memory(0x8020, &[0xff; 4], ReadWrite)
            .expect_memory(0x8020, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags(
            "POP FS:[ESP] uses next ESP for the destination offset",
            &[0x64, 0x8f, 0x04, 0x24],
        )
        .interpreter_only()
        .segment(Segment::Ss, data(0x8000, 0xff))
        .segment(Segment::Fs, data(0x4000, 0xff))
        .register(Esp, 0x20, 0x24)
        .memory(0x8020, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
        .memory(0x4020, &[0xff; 8], ReadWrite)
        .expect_memory(0x4024, &[0x78, 0x56, 0x34, 0x12]),
        Case::preserving_flags(
            "CALL return address is stored through SS",
            &[0xe8, 0xfb, 0x0f, 0, 0],
        )
        .interpreter_only()
        .segment(Segment::Ss, data(0x8000, 0xff))
        .register(Esp, 0x24, 0x20)
        .memory(0x8020, &[0xff; 4], ReadWrite)
        .expect_memory(0x8020, &[0x05, 0x10, 0, 0])
        .dispatch(0x2000),
        Case::preserving_flags("RET reads its target through SS", &[0xc3])
            .interpreter_only()
            .segment(Segment::Ss, data(0x8000, 0xff))
            .register(Esp, 0x20, 0x24)
            .memory(0x8020, &[0, 0x20, 0, 0], ReadOnly)
            .dispatch(0x2000),
        Case::preserving_flags(
            "flat PUSH writes a complete dword across linear zero",
            &[0x50],
        )
        .initial_register(Eax, 0x1234_5678)
        .register(Esp, 2, 0xffff_fffe)
        .memory(0xffff_fffe, &[0xff; 2], ReadWrite)
        .memory(0, &[0xff; 2], ReadWrite)
        .expect_memory(0xffff_fffe, &[0x78, 0x56])
        .expect_memory(0, &[0x34, 0x12]),
        Case::preserving_flags(
            "flat POP reads a complete dword across linear zero",
            &[0x58],
        )
        .register(Eax, 0, 0x1234_5678)
        .register(Esp, 0xffff_fffe, 2)
        .memory(0xffff_fffe, &[0x78, 0x56], ReadOnly)
        .memory(0, &[0x34, 0x12], ReadOnly),
    ]
}

fn restart_state() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, esp) in [
        (&[0x50][..], 0x23),
        (&[0x66, 0x50], 0x22),
        (&[0x58], 0x20),
        (&[0x9c], 0x23),
        (&[0x9d], 0x20),
        (&[0xe8, 0, 0, 0, 0], 0x23),
        (&[0xc3], 0x20),
    ] {
        cases.push(
            Case::preserving_flags(
                format!("SS limit fault preserves stack and flags {code:02x?}"),
                code,
            )
            .interpreter_only()
            .segment(Segment::Ss, data(0x8000, 0x20))
            .initial_register(Esp, esp)
            .memory(0x8010, &[0xff; 32], ReadWrite)
            .stack_fault(0),
        );
    }
    cases.push(
        Case::preserving_flags(
            "POP destination segment fault preserves the original ESP",
            &[0x64, 0x8f, 0x04, 0x24],
        )
        .interpreter_only()
        .segment(Segment::Ss, data(0x8000, 0xff))
        .segment(Segment::Fs, data(0x4000, 0x26))
        .initial_register(Esp, 0x20)
        .memory(0x8020, &[0x78, 0x56, 0x34, 0x12], ReadOnly)
        .memory(0x4020, &[0xff; 8], ReadWrite)
        .general_protection(0),
    );
    cases.push(
        Case::preserving_flags(
            "PUSH source page fault precedes destination segment fault",
            &[0x64, 0xff, 0x33],
        )
        .interpreter_only()
        .segment(Segment::Ss, data(0x8000, 0))
        .segment(Segment::Fs, data(0x4000, 0xff))
        .initial_registers(&[(Ebx, 0x20), (Esp, 0x24)])
        .fault(0x4020, 0),
    );
    cases
}

test_cases!(implicit_stack_accesses_use_ss, stack_transfers());
test_cases!(segment_faults_keep_stack_restart_state, restart_state());
