use crate::support::cases::Flags;
use wasm86_x86::{CpuState, StoredFlags};
use wasm86_x86::{FlagBytes, StoredStatusSource};

use crate::support::machine::{byte_register_image, Image};

#[path = "shifts/counts.rs"]
mod counts;
#[path = "shifts/decoding.rs"]
mod decoding;
#[path = "shifts/memory.rs"]
mod memory;
#[path = "shifts/sequences.rs"]
mod sequences;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Shl,
    Shr,
    Sar,
}

impl Operation {
    fn extension(self) -> u8 {
        match self {
            Self::Shl => 4,
            Self::Shr => 5,
            Self::Sar => 7,
        }
    }
}

const OPERATIONS: [Operation; 3] = [Operation::Shl, Operation::Shr, Operation::Sar];

const STORED_FLAGS: StoredFlags = StoredFlags {
    status_source: StoredStatusSource {
        kind: 9,
        reserved: [0xa5; 3],
        left: 0x7fff_fffe,
        right: 0xffff_fffe,
    },
    bytes: FlagBytes {
        cf: 0xfe,
        pf: 0x7f,
        af: 0x5a,
        zf: 0x80,
        sf: 0xff,
        of: 1,
        tf: 0,
        df: 1,
        nt: 0,
        ac: 0,
        id: 0,
        reserved: 0xa5,
    },
};

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags = STORED_FLAGS;
    image
}

struct ModelResult {
    value: u32,
    status: Option<Flags<u8>>,
}

impl ModelResult {
    fn apply_flags(&self, cpu: &mut CpuState) {
        if let Some(status) = self.status {
            cpu.flags.status_source.kind = 0;
            cpu.flags.bytes = FlagBytes {
                cf: status.cf,
                pf: status.pf,
                af: status.af,
                zf: status.zf,
                sf: status.sf,
                of: status.of,
                ..cpu.flags.bytes
            };
        }
    }
}

// Compute one-bit shifts with widened arithmetic rather than host shift operators.
// This keeps narrow operands and counts at or beyond their width independent of
// the generated Wasm carrier's shift rules.
fn bit_at_a_time_model(operation: Operation, bits: u32, value: u32, count: u8) -> ModelResult {
    let modulus = 1_u64 << bits;
    let sign = modulus / 2;
    let original = u64::from(value) % modulus;
    let count = u32::from(count & 31);
    let mut result = original;
    let mut carry = false;
    for _ in 0..count {
        match operation {
            Operation::Shl => {
                carry = result >= sign;
                result = result * 2 % modulus;
            }
            Operation::Shr => {
                carry = result % 2 != 0;
                result /= 2;
            }
            Operation::Sar => {
                carry = result % 2 != 0;
                result = result / 2 + if result >= sign { sign } else { 0 };
            }
        }
    }
    if count == 0 {
        return ModelResult {
            value: result as u32,
            status: None,
        };
    }
    // Defined emulator policy for flags the architecture leaves unspecified.
    if operation != Operation::Sar && count >= bits {
        carry = false;
    }
    let overflow = count == 1
        && match operation {
            Operation::Shl => (result >= sign) != carry,
            Operation::Shr => original >= sign,
            Operation::Sar => false,
        };
    ModelResult {
        value: result as u32,
        status: Some(Flags {
            cf: u8::from(carry),
            pf: u8::from((result as u8).count_ones() % 2 == 0),
            af: 0,
            zf: u8::from(result == 0),
            sf: u8::from(result >= sign),
            of: u8::from(overflow),
        }),
    }
}
