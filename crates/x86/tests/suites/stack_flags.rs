//! Stack flag images follow the flat user-mode model and the default 32-bit stack.

#[path = "stack_flags/decoding.rs"]
mod decoding;
#[path = "stack_flags/images.rs"]
mod images;
#[path = "stack_flags/memory.rs"]
mod memory;
#[path = "stack_flags/sequences.rs"]
mod sequences;

use crate::flags::Flag;
use crate::support::cases::{
    FlagExpectation::{self, Clear, Preserved, Set},
    Flags, InstructionCase as Case,
};
use wasm86_x86::{CpuState, FlagBytes, StoredFlags, StoredStatusSource};

#[derive(Clone, Copy)]
struct FlagImage {
    name: &'static str,
    bits: u32,
    status: u8,
    direct: u8,
}

// The dense fixture masks are CF/PF/AF/ZF/SF/OF and TF/DF/NT/AC/ID.
// Architectural image values are literal, independently of those fixture positions.
#[rustfmt::skip]
const IMAGES: [FlagImage; 15] = [
    FlagImage { name: "clear", bits: 0x0000_0202, status: 0, direct: 0 },
    FlagImage { name: "CF", bits: 0x0000_0203, status: 1, direct: 0 },
    FlagImage { name: "PF", bits: 0x0000_0206, status: 2, direct: 0 },
    FlagImage { name: "AF", bits: 0x0000_0212, status: 4, direct: 0 },
    FlagImage { name: "ZF", bits: 0x0000_0242, status: 8, direct: 0 },
    FlagImage { name: "SF", bits: 0x0000_0282, status: 16, direct: 0 },
    FlagImage { name: "OF", bits: 0x0000_0a02, status: 32, direct: 0 },
    FlagImage { name: "TF", bits: 0x0000_0302, status: 0, direct: 1 },
    FlagImage { name: "DF", bits: 0x0000_0602, status: 0, direct: 2 },
    FlagImage { name: "NT", bits: 0x0000_4202, status: 0, direct: 4 },
    FlagImage { name: "AC", bits: 0x0004_0202, status: 0, direct: 8 },
    FlagImage { name: "ID", bits: 0x0020_0202, status: 0, direct: 16 },
    FlagImage { name: "all", bits: 0x0024_4fd7, status: 63, direct: 31 },
    FlagImage { name: "alternating", bits: 0x0020_4393, status: 21, direct: 21 },
    FlagImage { name: "complementary", bits: 0x0004_0e46, status: 42, direct: 10 },
];

fn logical_flags(bits: u8) -> Flags<bool> {
    Flags {
        cf: bits & 1 != 0,
        pf: bits & 2 != 0,
        af: bits & 4 != 0,
        zf: bits & 8 != 0,
        sf: bits & 16 != 0,
        of: bits & 32 != 0,
    }
}

fn status_expectations(bits: u8) -> Flags<FlagExpectation> {
    let bit = |mask| if bits & mask != 0 { Set } else { Clear };
    Flags {
        cf: bit(1),
        pf: bit(2),
        af: bit(4),
        zf: bit(8),
        sf: bit(16),
        of: bit(32),
    }
}

fn stored_flags(status: u8, direct: u8) -> StoredFlags {
    StoredFlags {
        status_source: StoredStatusSource {
            kind: 0,
            reserved: [0x5a, 0xc3, 0x96],
            left: 0x1234_5678,
            right: 0x8765_4321,
        },
        bytes: FlagBytes {
            cf: 0x80 | (status & 1),
            pf: 0x5a | ((status >> 1) & 1),
            af: 0xfe | ((status >> 2) & 1),
            zf: 0xc2 | ((status >> 3) & 1),
            sf: 0x3c | ((status >> 4) & 1),
            of: 0x96 | ((status >> 5) & 1),
            tf: 0x80 | (direct & 1),
            df: 0xfe | ((direct >> 1) & 1),
            nt: 0x5a | ((direct >> 2) & 1),
            ac: 0xc2 | ((direct >> 3) & 1),
            id: 0x3c | ((direct >> 4) & 1),
            ..CpuState::filled(0xa5).flags.bytes
        },
    }
}

fn push_case(name: impl Into<String>, code: &[u8], image: FlagImage) -> Case {
    Case::new(
        name,
        code,
        logical_flags(image.status),
        Flags::all(Preserved),
    )
    .stored_flags(stored_flags(image.status, image.direct))
    .preserve_flag_record()
}

fn pop_case(name: impl Into<String>, code: &[u8], image: FlagImage, word: bool) -> Case {
    let mut case = Case::new(
        name,
        code,
        logical_flags(image.status ^ 63),
        status_expectations(image.status),
    )
    .stored_flags(stored_flags(image.status ^ 63, image.direct ^ 31))
    .expect_direct_flag(Flag::TF, image.direct & 1 != 0)
    .expect_direct_flag(Flag::DF, image.direct & 2 != 0)
    .expect_direct_flag(Flag::NT, image.direct & 4 != 0);
    if !word {
        case = case
            .expect_direct_flag(Flag::AC, image.direct & 8 != 0)
            .expect_direct_flag(Flag::ID, image.direct & 16 != 0);
    }
    case
}
