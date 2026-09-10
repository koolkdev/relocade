use wasm86_x86::{CpuState, StatusFlags};

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

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags.kind = 9;
    image.cpu.flags.left = 0x7fff_fffe;
    image.cpu.flags.right = 0xffff_fffe;
    image.cpu.flags.status = StatusFlags {
        cf: 0xfe,
        pf: 0x7f,
        af: 0x5a,
        zf: 0x80,
        sf: 0xff,
        of: 1,
    };
    image.cpu.flags.non_status = [0, 1, 0, 0, 0, 0xa5];
    image
}

struct Expected {
    value: u32,
    status: Option<StatusFlags>,
}

impl Expected {
    fn apply_flags(&self, cpu: &mut CpuState) {
        if let Some(status) = self.status {
            cpu.flags.kind = 0;
            cpu.flags.status = status;
        }
    }
}

// Compute one-bit shifts with widened arithmetic rather than host shift operators.
// This keeps narrow operands and counts at or beyond their width independent of
// the generated Wasm carrier's shift rules.
fn expected(operation: Operation, bits: u32, value: u32, count: u8) -> Expected {
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
        return Expected {
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
    Expected {
        value: result as u32,
        status: Some(StatusFlags {
            cf: u8::from(carry),
            pf: u8::from((result as u8).count_ones() % 2 == 0),
            af: 0,
            zf: u8::from(result == 0),
            sf: u8::from(result >= sign),
            of: u8::from(overflow),
        }),
    }
}
