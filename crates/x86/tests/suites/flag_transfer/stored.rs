use super::{byte_flags, concrete_record, sahf_flags, Case, Flags, Preserved};
use crate::support::cases::test_cases;
use wasm86_x86::{Gpr32::Eax, StoredFlags, StoredStatusSource};

fn stored_sources() -> Vec<Case> {
    let mut cases = Vec::new();
    // Literal results include width-discarded operand bits and an irrelevant logical RHS.
    for (name, kind, left, right, ah, overflow) in [
        ("concrete", 0, 0x1234_5678, 0x8765_4321, 0x93, true),
        ("byte SUB", 1, 0xabcd_0000, 0x1234_0001, 0x97, false),
        ("byte ADD", 2, 0xabcd_007f, 0x1234_0001, 0x92, true),
        ("byte logical", 3, 0xabcd_0080, 0xdead_beef, 0x82, false),
        ("word SUB", 5, 0xabcd_8000, 0x1234_0001, 0x16, true),
        ("word ADD", 6, 0xabcd_ffff, 0x1234_0001, 0x57, false),
        ("word logical", 7, 0xabcd_8000, 0xdead_beef, 0x86, false),
        ("dword SUB", 9, 0x8000_0000, 1, 0x16, true),
        ("dword ADD", 10, 0xffff_ffff, 1, 0x57, false),
        ("dword logical", 11, 0, 0xdead_beef, 0x46, false),
    ] {
        let flags = byte_flags(ah, overflow);
        let record = StoredFlags {
            status_source: StoredStatusSource {
                kind,
                left,
                right,
                ..concrete_record(flags, 0x5a).status_source
            },
            ..concrete_record(
                if kind == 0 {
                    flags
                } else {
                    byte_flags(!ah, !overflow)
                },
                0x5a,
            )
        };
        cases.push(
            Case::new(
                format!("LAHF resolves {name} and preserves its complete record"),
                &[0x9f],
                flags,
                Flags::all(Preserved),
            )
            .stored_flags(record)
            .preserve_flag_record()
            .register(Eax, 0x4433_ff11, 0x4433_0011 | (u32::from(ah) << 8)),
        );
        cases.push(
            Case::new(
                format!("SAHF replaces {name} flags and preserves its overflow"),
                &[0x9e],
                flags,
                sahf_flags(0x2d),
            )
            .stored_flags(record)
            .initial_register(Eax, 0x4433_2d11),
        );
    }
    cases
}

test_cases!(
    transfers_consume_concrete_and_all_arithmetic_source_kinds,
    stored_sources()
);
