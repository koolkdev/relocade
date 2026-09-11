use wasm86_x86::{CpuState, Gpr32::Eax, StatusFlags, StoredFlags};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Preserved, Set},
        Flags, InstructionCase as Case,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

#[path = "unary_operations/carry.rs"]
mod carry;
#[path = "unary_operations/cases.rs"]
mod cases;
#[path = "unary_operations/decoding.rs"]
mod decoding;
#[path = "unary_operations/memory.rs"]
mod memory;
#[path = "unary_operations/registers.rs"]
mod registers;

#[rustfmt::skip]
fn negation_conditions() -> Vec<SequenceCase> {
    vec![
        SequenceCase::from_opaque_flags("NEG 8, initial EAX 0x12345600")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12345600)
            .step(Checkpoint::new(&[0xf6, 0xd8], Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x12345600))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 16, initial EAX 0x12340000")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12340000)
            .step(Checkpoint::new(&[0x66, 0xf7, 0xd8], Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x12340000))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 32, initial EAX 0x00000000")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x00000000)
            .step(Checkpoint::new(&[0xf7, 0xd8], Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x00000000))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 8, initial EAX 0x12345601")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12345601)
            .step(Checkpoint::new(&[0xf6, 0xd8], Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x123456ff))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 16, initial EAX 0x12340001")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12340001)
            .step(Checkpoint::new(&[0x66, 0xf7, 0xd8], Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x1234ffff))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 32, initial EAX 0x00000001")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x00000001)
            .step(Checkpoint::new(&[0xf7, 0xd8], Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0xffffffff))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 8, initial EAX 0x12345610")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12345610)
            .step(Checkpoint::new(&[0xf6, 0xd8], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x123456f0))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 16, initial EAX 0x12340010")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12340010)
            .step(Checkpoint::new(&[0x66, 0xf7, 0xd8], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x1234fff0))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 32, initial EAX 0x00000010")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x00000010)
            .step(Checkpoint::new(&[0xf7, 0xd8], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear }).register(Eax, 0xfffffff0))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 8, initial EAX 0x1234567f")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x1234567f)
            .step(Checkpoint::new(&[0xf6, 0xd8], Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x12345681))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 16, initial EAX 0x12347fff")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12347fff)
            .step(Checkpoint::new(&[0x66, 0xf7, 0xd8], Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x12348001))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 32, initial EAX 0x7fffffff")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x7fffffff)
            .step(Checkpoint::new(&[0xf7, 0xd8], Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear }).register(Eax, 0x80000001))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0]),
        SequenceCase::from_opaque_flags("NEG 8, initial EAX 0x12345680")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12345680)
            .step(Checkpoint::new(&[0xf6, 0xd8], Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set }).register(Eax, 0x12345680))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1]),
        SequenceCase::from_opaque_flags("NEG 16, initial EAX 0x12348000")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x12348000)
            .step(Checkpoint::new(&[0x66, 0xf7, 0xd8], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set }).register(Eax, 0x12348000))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1]),
        SequenceCase::from_opaque_flags("NEG 32, initial EAX 0x80000000")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x80000000)
            .step(Checkpoint::new(&[0xf7, 0xd8], Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set }).register(Eax, 0x80000000))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 0, 1]),
        SequenceCase::from_opaque_flags("NEG 8, initial EAX 0x123456ff")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x123456ff)
            .step(Checkpoint::new(&[0xf6, 0xd8], Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x12345601))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::from_opaque_flags("NEG 16, initial EAX 0x1234ffff")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0x1234ffff)
            .step(Checkpoint::new(&[0x66, 0xf7, 0xd8], Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x12340001))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::from_opaque_flags("NEG 32, initial EAX 0xffffffff")
            .stored_flags(StoredFlags { kind: 0xff, ..CpuState::filled(0xa5).flags })
            .initial_register(Eax, 0xffffffff)
            .step(Checkpoint::new(&[0xf7, 0xd8], Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear }).register(Eax, 0x00000001))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 0, 1, 0, 1, 0, 1, 0, 1]),
    ]
}

test_sequences!(negation_replaces_invalid_flags, negation_conditions());

fn not_preserves_flag_records() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, input, output) in [
        (&[0xf6, 0xd0][..], 0x1234_5600, 0x1234_56ff),
        (&[0x66, 0xf7, 0xd0][..], 0x1234_0000, 0x1234_ffff),
        (&[0xf7, 0xd0][..], 0x1234_5678, 0xedcb_a987),
    ] {
        for kind in [0, 2, 7, 9, 0xff] {
            let stored = StoredFlags {
                kind,
                left: 0x0123_4567,
                right: 0x89ab_cdef,
                status: StatusFlags {
                    cf: 0x80,
                    pf: 0x81,
                    af: 0xfe,
                    zf: 0xff,
                    sf: 0x55,
                    of: 0xaa,
                },
                ..CpuState::filled(0xa5).flags
            };
            cases.push(
                Case::preserving_flags(format!("NOT {code:02x?}; stored kind {kind}"), code)
                    .stored_flags(stored)
                    .register(Eax, input, output),
            );
        }
    }
    cases
}

test_cases!(not_preserves_every_flag_byte, not_preserves_flag_records());

#[rustfmt::skip]
fn carry_and_unary_conditions() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new("INC AL preserves clear carry", Flags { cf: false, ..Flags::all(true) })
            .initial_register(Eax, 0x1234_56ff)
            .step(Checkpoint::new(&[0xfe, 0xc0], Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .register(Eax, 0x1234_5600))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("INC AL preserves set carry", Flags::all(true))
            .initial_register(Eax, 0x1234_56ff)
            .step(Checkpoint::new(&[0xfe, 0xc0], Flags { cf: Preserved, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .register(Eax, 0x1234_5600))
            .conditions([0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("DEC AL preserves clear carry", Flags { cf: false, ..Flags::all(true) })
            .initial_register(Eax, 0x1234_5600)
            .step(Checkpoint::new(&[0xfe, 0xc8], Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
                .register(Eax, 0x1234_56ff))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("DEC AL preserves set carry", Flags::all(true))
            .initial_register(Eax, 0x1234_5600)
            .step(Checkpoint::new(&[0xfe, 0xc8], Flags { cf: Preserved, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
                .register(Eax, 0x1234_56ff))
            .conditions([0, 1, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0]),
    ]
}

test_sequences!(
    setcc_combines_preserved_carry_and_unary_result,
    carry_and_unary_conditions()
);
