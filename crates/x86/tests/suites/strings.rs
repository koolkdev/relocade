//! Single-iteration string operations use flat, 32-bit indices and the stored DF bit.

#[path = "strings/decoding.rs"]
mod decoding;
#[path = "strings/memory.rs"]
mod memory;
#[path = "strings/sequences.rs"]
mod sequences;
#[path = "strings/values.rs"]
mod values;

use crate::support::cases::{
    FlagExpectation::{self, Clear, Set},
    Flags,
};
use wasm86_x86::{CpuState, StoredFlags, StoredStatusSource};

#[derive(Clone, Copy, Debug, PartialEq)]
enum Operation {
    Movs,
    Cmps,
    Stos,
    Lods,
    Scas,
}

const OPERATIONS: [Operation; 5] = [
    Operation::Movs,
    Operation::Cmps,
    Operation::Stos,
    Operation::Lods,
    Operation::Scas,
];

impl Operation {
    fn code(self, width: u32) -> Vec<u8> {
        let byte = match self {
            Self::Movs => 0xa4,
            Self::Cmps => 0xa6,
            Self::Stos => 0xaa,
            Self::Lods => 0xac,
            Self::Scas => 0xae,
        };
        match width {
            1 => vec![byte],
            2 => vec![0x66, byte + 1],
            4 => vec![byte + 1],
            _ => unreachable!(),
        }
    }
    fn uses_source_index(self) -> bool {
        matches!(self, Self::Movs | Self::Cmps | Self::Lods)
    }
    fn uses_destination_index(self) -> bool {
        !matches!(self, Self::Lods)
    }
    fn writes_memory(self) -> bool {
        matches!(self, Self::Movs | Self::Stos)
    }
    fn compares(self) -> bool {
        matches!(self, Self::Cmps | Self::Scas)
    }
}

fn flags(bits: u8) -> Flags<FlagExpectation> {
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

fn record(df: u8) -> StoredFlags {
    let mut record = CpuState::filled(0xa5).flags;
    record.status_source = StoredStatusSource {
        kind: 0,
        reserved: [0x5a, 0xc3, 0x96],
        left: 0x1234_5678,
        right: 0x8765_4321,
    };
    record.bytes.df = df;
    record.bytes.tf = 0x80;
    record.bytes.nt = 0xff;
    record.bytes.ac = 0xfe;
    record.bytes.id = 0x81;
    record
}
