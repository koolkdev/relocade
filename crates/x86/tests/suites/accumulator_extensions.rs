use wasm86_x86::{FlagBytes, StoredStatusSource};
#[path = "accumulator_extensions/decoding.rs"]
mod decoding;
#[path = "accumulator_extensions/sequences.rs"]
mod sequences;

use crate::support::cases::{
    test_cases, FlagExpectation::Preserved, Flags, InstructionCase as Case,
};
use wasm86_x86::{
    CpuState,
    Gpr32::{Eax, Edx},
    StoredFlags,
};

const ENCODINGS: [(&str, &[u8]); 4] = [
    ("CBW", &[0x66, 0x98]),
    ("CWDE", &[0x98]),
    ("CWD", &[0x66, 0x99]),
    ("CDQ", &[0x99]),
];

#[rustfmt::skip]
fn boundary_cases() -> Vec<Case> {
    [
        ("CBW zero", &[0x66, 0x98][..], 0x4433_ff00, 0x4433_0000, 0xccbb_aa99),
        ("CBW positive maximum", &[0x66, 0x98], 0x4433_807f, 0x4433_007f, 0xccbb_aa99),
        ("CBW negative minimum", &[0x66, 0x98], 0x4433_7f80, 0x4433_ff80, 0xccbb_aa99),
        ("CBW negative one", &[0x66, 0x98], 0x4433_00ff, 0x4433_ffff, 0xccbb_aa99),
        ("CWDE zero", &[0x98], 0xffff_0000, 0, 0xccbb_aa99),
        ("CWDE positive maximum", &[0x98], 0xffff_7fff, 0x0000_7fff, 0xccbb_aa99),
        ("CWDE negative minimum", &[0x98], 0x0000_8000, 0xffff_8000, 0xccbb_aa99),
        ("CWDE negative one", &[0x98], 0x1234_ffff, 0xffff_ffff, 0xccbb_aa99),
        ("CWD zero", &[0x66, 0x99], 0xffff_0000, 0xffff_0000, 0xccbb_0000),
        ("CWD positive maximum", &[0x66, 0x99], 0xffff_7fff, 0xffff_7fff, 0xccbb_0000),
        ("CWD negative minimum", &[0x66, 0x99], 0x0000_8000, 0x0000_8000, 0xccbb_ffff),
        ("CWD negative one", &[0x66, 0x99], 0x1234_ffff, 0x1234_ffff, 0xccbb_ffff),
        ("CDQ zero", &[0x99], 0, 0, 0),
        ("CDQ positive maximum", &[0x99], 0x7fff_ffff, 0x7fff_ffff, 0),
        ("CDQ negative minimum", &[0x99], 0x8000_0000, 0x8000_0000, 0xffff_ffff),
        ("CDQ negative one", &[0x99], 0xffff_ffff, 0xffff_ffff, 0xffff_ffff),
    ].into_iter().map(|(name, code, input, eax, edx)| {
        Case::preserving_flags(name, code)
            .register(Eax, input, eax).register(Edx, 0xccbb_aa99, edx)
    }).collect()
}

fn stored_flag_cases() -> Vec<Case> {
    let concrete = StoredFlags {
        status_source: StoredStatusSource {
            kind: 0,
            left: 0x1234_5678,
            right: 0x8765_4321,
            ..(CpuState::filled(0xa5).flags).status_source
        },
        bytes: FlagBytes {
            cf: 1,
            pf: 0,
            af: 1,
            zf: 0,
            sf: 1,
            of: 0,
            ..(CpuState::filled(0xa5).flags).bytes
        },
    };
    let pending_add = StoredFlags {
        status_source: StoredStatusSource {
            kind: 10,
            left: 0x7fff_ffff,
            right: 1,
            ..(CpuState::filled(0x5a).flags).status_source
        },
        bytes: FlagBytes {
            cf: 1,
            pf: 0,
            af: 0,
            zf: 1,
            sf: 0,
            of: 0,
            ..(CpuState::filled(0x5a).flags).bytes
        },
    };
    let mut cases = Vec::new();
    for (name, stored, flags) in [
        (
            "concrete flags with unused payload",
            concrete,
            Flags {
                cf: true,
                pf: false,
                af: true,
                zf: false,
                sf: true,
                of: false,
            },
        ),
        (
            "pending ADD with contradictory concrete bytes",
            pending_add,
            Flags {
                cf: false,
                pf: true,
                af: true,
                zf: false,
                sf: true,
                of: true,
            },
        ),
    ] {
        for ((mnemonic, code), (eax, edx)) in ENCODINGS.into_iter().zip([
            (0x1234_ff81, 0xccbb_aa99),
            (0xffff_8081, 0xccbb_aa99),
            (0x1234_8081, 0xccbb_ffff),
            (0x1234_8081, 0),
        ]) {
            cases.push(
                Case::new(
                    format!("{mnemonic}: {name}"),
                    code,
                    flags,
                    Flags::all(Preserved),
                )
                .stored_flags(stored)
                .preserve_flag_record()
                .register(Eax, 0x1234_8081, eax)
                .register(Edx, 0xccbb_aa99, edx),
            );
        }
    }
    cases
}

test_cases!(
    sign_boundaries_preserve_unwritten_register_bits,
    boundary_cases()
);
test_cases!(concrete_and_lazy_flags_are_unchanged, stored_flag_cases());
