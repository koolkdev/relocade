use crate::support::cases::Flags;
use wasm86_x86::{CpuState, StoredFlags};
use wasm86_x86::{FlagBytes, StoredStatusSource};

use crate::support::machine::{byte_register_image, Exit, Image, Step};

#[path = "carry_rotates/counts.rs"]
mod counts;
#[path = "carry_rotates/decoding.rs"]
mod decoding;
#[path = "carry_rotates/flags.rs"]
mod flags;
#[path = "carry_rotates/full_rings.rs"]
mod full_rings;
#[path = "carry_rotates/memory.rs"]
mod memory;
#[path = "carry_rotates/sequences.rs"]
mod sequences;

#[derive(Clone, Copy, Debug)]
enum Operation {
    Rcl,
    Rcr,
}

impl Operation {
    fn extension(self) -> u8 {
        match self {
            Self::Rcl => 2,
            Self::Rcr => 3,
        }
    }
}

const OPERATIONS: [Operation; 2] = [Operation::Rcl, Operation::Rcr];

fn prior_flags(carry: u8) -> Flags<u8> {
    Flags {
        cf: carry,
        pf: 0,
        af: 1,
        zf: 1,
        sf: 0,
        of: 1,
    }
}

fn stored_flags(carry: u8) -> StoredFlags {
    StoredFlags {
        status_source: StoredStatusSource {
            kind: 0,
            reserved: [0xa5; 3],
            left: 0x1234_5678,
            right: 0x8765_4321,
        },
        bytes: FlagBytes {
            cf: 0xfe | carry,
            pf: 0xfe,
            af: 0xff,
            zf: 0x7f,
            sf: 0x80,
            of: 0x5b,
            tf: 0,
            df: 1,
            nt: 0,
            ac: 0,
            id: 0,
            reserved: 0xa5,
        },
    }
}

fn image(code: &[u8], carry: u8) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags = stored_flags(carry);
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

// Move one bit through the operand and carry at a time. Complete carry rings
// preserve the stored flags. For other counts, only masked one defines OF;
// this implementation chooses zero for the remaining undefined OF values.
fn bit_at_a_time_model(
    operation: Operation,
    bits: u32,
    value: u32,
    count: u8,
    prior: Flags<u8>,
) -> ModelResult {
    let modulus = 2_u64.pow(bits);
    let sign = modulus / 2;
    let mut result = u64::from(value) % modulus;
    let count = count & 31;
    let mut carry = prior.cf != 0;
    for _ in 0..count {
        let incoming = carry;
        match operation {
            Operation::Rcl => {
                carry = result >= sign;
                result = result * 2 % modulus + u64::from(incoming);
            }
            Operation::Rcr => {
                carry = result % 2 != 0;
                result = result / 2 + if incoming { sign } else { 0 };
            }
        }
    }
    let overflow = count == 1
        && match operation {
            Operation::Rcl => (result >= sign) != carry,
            Operation::Rcr => (result >= sign) != (result % sign >= sign / 2),
        };
    ModelResult {
        value: result as u32,
        status: (u32::from(count) % (bits + 1) != 0).then_some(Flags {
            cf: u8::from(carry),
            of: u8::from(overflow),
            ..prior
        }),
    }
}

fn retire(cpu: &mut CpuState, length: u32, ram: &'static [(u32, &'static [u8])]) -> Step<'static> {
    cpu.eip += length;
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    Step {
        cpu: *cpu,
        ram,
        exit: Exit::Dispatch(cpu.eip),
    }
}
