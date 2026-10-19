use super::data;
use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadWrite};
use relocade_x86::{
    Gpr32::{Eax, Ebp, Ebx},
    Segment, StoredSegment,
};

fn access_rights() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, usable, executable, readable, writable) in [
        ("read-only data", 1, 0, true, false),
        ("writable data", 1, 0, true, true),
        ("execute-only code", 1, 1, false, false),
        ("readable code", 1, 1, true, false),
        ("unusable", 0, 0, false, false),
    ] {
        let cache = StoredSegment {
            usable,
            executable,
            readable: u8::from(readable),
            writable: u8::from(writable),
            ..data(0x8000, 0xff)
        };
        let read = Case::preserving_flags(format!("FS read rights {name}"), &[0x64, 0x8b, 0x03])
            .segment(Segment::Fs, cache)
            .initial_register(Ebx, 0x20)
            .memory(0x8020, &[0x78, 0x56, 0x34, 0x12], ReadWrite);
        cases.push(if readable {
            read.register(Eax, 0, 0x1234_5678)
        } else {
            read.general_protection(0)
        });
        let write = Case::preserving_flags(format!("FS write rights {name}"), &[0x64, 0x89, 0x03])
            .segment(Segment::Fs, cache)
            .initial_registers(&[(Eax, 0x8765_4321), (Ebx, 0x20)])
            .memory(0x8020, &[0x78, 0x56, 0x34, 0x12], ReadWrite);
        cases.push(if writable {
            write.expect_memory(0x8020, &[0x21, 0x43, 0x65, 0x87])
        } else {
            write.general_protection(0)
        });
    }
    for readable in [true, false] {
        let cs = StoredSegment {
            readable: u8::from(readable),
            ..StoredSegment::flat_code32(0x1b)
        };
        let read = Case::preserving_flags(
            format!("CS read permission {readable}"),
            &[0x2e, 0x8b, 0x03],
        )
        .segment(Segment::Cs, cs)
        .initial_register(Ebx, 0x4000)
        .memory(0x4000, &[0x78, 0x56, 0x34, 0x12], ReadWrite);
        cases.push(if readable {
            read.register(Eax, 0, 0x1234_5678)
        } else {
            read.segmented_only().general_protection(0)
        });
        let write = Case::preserving_flags(
            format!("CS never permits writes {readable}"),
            &[0x2e, 0x89, 0x03],
        )
        .segment(Segment::Cs, cs)
        .initial_register(Ebx, 0x4000)
        .memory(0x4000, &[0x78, 0x56, 0x34, 0x12], ReadWrite)
        .general_protection(0);
        cases.push(if readable {
            write
        } else {
            write.segmented_only()
        });
    }
    cases
}

fn fault_selection() -> Vec<Case> {
    use crate::support::cases::{FlagExpectation::Preserved, Flags};
    let mut cases = vec![
        Case::new(
            "false CMOV still checks its source segment",
            &[0x64, 0x0f, 0x44, 0x03],
            Flags::all(false),
            Flags::all(Preserved),
        )
        .segment(Segment::Fs, StoredSegment::unusable(0x53))
        .initial_register(Ebx, 0x4000)
        .general_protection(0),
        Case::preserving_flags("unusable DS faults before an absent page", &[0x8b, 0x03])
            .segmented_only()
            .segment(Segment::Ds, StoredSegment::unusable(0x23))
            .initial_register(Ebx, 0x4000)
            .general_protection(0),
        Case::preserving_flags(
            "SS override reports stack fault for an ordinary operand",
            &[0x36, 0x8b, 0x03],
        )
        .segmented_only()
        .segment(Segment::Ss, data(0x8000, 0x20))
        .initial_register(Ebx, 0x20)
        .stack_fault(0),
        Case::preserving_flags(
            "DS override on EBP reports general protection",
            &[0x3e, 0x8b, 0x45, 0],
        )
        .segmented_only()
        .segment(Segment::Ds, data(0x8000, 0x20))
        .initial_register(Ebp, 0x20)
        .general_protection(0),
        Case::preserving_flags(
            "unusable SS with retained B=1 faults on access",
            &[0x36, 0x8b, 0x03],
        )
        .segmented_only()
        .segment(
            Segment::Ss,
            StoredSegment {
                big: 1,
                ..StoredSegment::unusable(0x23)
            },
        )
        .initial_register(Ebx, 0)
        .stack_fault(0),
        Case::preserving_flags(
            "unusable SS with retained B=1 does not fault without an access",
            &[0x8b, 0xc3],
        )
        .segmented_only()
        .segment(
            Segment::Ss,
            StoredSegment {
                big: 1,
                ..StoredSegment::unusable(0x23)
            },
        )
        .initial_register(Ebx, 0x1234_5678)
        .register(Eax, 0, 0x1234_5678),
    ];
    for code in [
        &[0x64, 0x83, 0x03, 1][..],
        &[0x64, 0xff, 0x03],
        &[0x64, 0x87, 0x03],
        &[0x64, 0x0f, 0xc1, 0x03],
        &[0x64, 0x0f, 0xab, 0x0b],
    ] {
        cases.push(
            Case::preserving_flags(
                format!("write rights fault preserves RMW state {code:02x?}"),
                code,
            )
            .segment(
                Segment::Fs,
                StoredSegment {
                    writable: 0,
                    ..data(0x8000, 0xff)
                },
            )
            .initial_registers(&[(Ebx, 0x20), (relocade_x86::Gpr32::Ecx, 0)])
            .memory(0x8020, &[0xff; 4], ReadWrite)
            .general_protection(0),
        );
    }
    cases
}

test_cases!(loaded_type_controls_reads_and_writes, access_rights());
test_cases!(
    fault_kind_and_atomicity_follow_actual_access,
    fault_selection()
);
