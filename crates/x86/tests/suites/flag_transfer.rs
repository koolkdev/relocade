//! AH transfers use the architectural flag-byte positions, independent of operand size.

#[path = "flag_transfer/decoding.rs"]
mod decoding;
#[path = "flag_transfer/sequences.rs"]
mod sequences;
#[path = "flag_transfer/stored.rs"]
mod stored;

use crate::support::cases::{
    test_cases,
    FlagExpectation::{self, Clear, Preserved, Set},
    Flags, InstructionCase as Case,
};
use wasm86_x86::{CpuState, FlagBytes, Gpr32::Eax, StoredFlags, StoredStatusSource};

// Ordered by CF, PF, AF, ZF, SF as a five-bit counter. The fixed bits are literal.
const LAHF_BYTES: [u8; 32] = [
    0x02, 0x03, 0x06, 0x07, 0x12, 0x13, 0x16, 0x17, 0x42, 0x43, 0x46, 0x47, 0x52, 0x53, 0x56, 0x57,
    0x82, 0x83, 0x86, 0x87, 0x92, 0x93, 0x96, 0x97, 0xc2, 0xc3, 0xc6, 0xc7, 0xd2, 0xd3, 0xd6, 0xd7,
];

fn byte_flags(ah: u8, overflow: bool) -> Flags<bool> {
    Flags {
        cf: ah & 0x01 != 0,
        pf: ah & 0x04 != 0,
        af: ah & 0x10 != 0,
        zf: ah & 0x40 != 0,
        sf: ah & 0x80 != 0,
        of: overflow,
    }
}

fn sahf_flags(ah: u8) -> Flags<FlagExpectation> {
    let bit = |mask| if ah & mask != 0 { Set } else { Clear };
    Flags {
        cf: bit(0x01),
        pf: bit(0x04),
        af: bit(0x10),
        zf: bit(0x40),
        sf: bit(0x80),
        of: Preserved,
    }
}

fn concrete_record(flags: Flags<bool>, direction: u8) -> StoredFlags {
    StoredFlags {
        status_source: StoredStatusSource {
            kind: 0,
            reserved: [0x5a, 0xc3, 0x96],
            left: 0x1234_5678,
            right: 0x8765_4321,
        },
        bytes: FlagBytes {
            cf: 0x80 | u8::from(flags.cf),
            pf: 0x5a | u8::from(flags.pf),
            af: 0xfe | u8::from(flags.af),
            zf: 0xc2 | u8::from(flags.zf),
            sf: 0x3c | u8::from(flags.sf),
            of: 0x96 | u8::from(flags.of),
            df: direction,
            ..CpuState::filled(0xa5).flags.bytes
        },
    }
}

fn lahf_combinations() -> Vec<Case> {
    let mut cases = Vec::new();
    for (bits, ah) in LAHF_BYTES.into_iter().enumerate() {
        for overflow in [false, true] {
            let flags = Flags {
                cf: bits & 1 != 0,
                pf: bits & 2 != 0,
                af: bits & 4 != 0,
                zf: bits & 8 != 0,
                sf: bits & 16 != 0,
                of: overflow,
            };
            cases.push(
                Case::new(
                    format!("LAHF flags {bits:05b}, OF {overflow}"),
                    &[0x9f],
                    flags,
                    Flags::all(Preserved),
                )
                .stored_flags(concrete_record(flags, if overflow { 0xfe } else { 0x81 }))
                .preserve_flag_record()
                .register(Eax, 0x4433_ff11, 0x4433_0011 | (u32::from(ah) << 8)),
            );
        }
    }
    cases
}

fn sahf_all_bytes() -> Vec<Case> {
    let mut cases = Vec::new();
    for ah in 0..=u8::MAX {
        for overflow in [false, true] {
            let flags = byte_flags(!ah, overflow);
            cases.push(
                Case::new(
                    format!("SAHF AH {ah:02x}, prior OF {overflow} ignores bits 1, 3 and 5"),
                    &[0x9e],
                    flags,
                    sahf_flags(ah),
                )
                .stored_flags(concrete_record(
                    flags,
                    if ah & 1 == 0 { 0xfe } else { 0x81 },
                ))
                .initial_register(Eax, 0x4433_00a5 | (u32::from(ah) << 8)),
            );
        }
    }
    cases
}

test_cases!(
    lahf_packs_all_flag_combinations_without_changing_backing,
    lahf_combinations()
);
test_cases!(sahf_replaces_five_flags_for_every_ah_byte, sahf_all_bytes());
