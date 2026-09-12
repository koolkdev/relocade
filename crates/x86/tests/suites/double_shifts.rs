use crate::support::cases::Flags;
use wasm86_x86::{CpuState, StoredFlags};
use wasm86_x86::{FlagBytes, StoredStatusSource};

use crate::support::machine::{byte_register_image, Exit, Image, Step};

#[path = "double_shifts/cases.rs"]
mod cases;

#[path = "double_shifts/counts.rs"]
mod counts;
#[path = "double_shifts/decoding.rs"]
mod decoding;
#[path = "double_shifts/memory.rs"]
mod memory;
#[path = "double_shifts/sequences.rs"]
mod sequences;

#[derive(Clone, Copy, Debug)]
enum Operation {
    Shld,
    Shrd,
}

impl Operation {
    fn opcode(self, from_cl: bool) -> u8 {
        (match self {
            Self::Shld => 0xa4,
            Self::Shrd => 0xac,
        }) + u8::from(from_cl)
    }
}

const OPERATIONS: [Operation; 2] = [Operation::Shld, Operation::Shrd];

const STORED_FLAGS: StoredFlags = StoredFlags {
    status_source: StoredStatusSource {
        kind: 6,
        reserved: [0xa5; 3],
        left: 0x7fff,
        right: 1,
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

// Shift one destination bit at a time, injecting successive source bits.
// This oracle does not use the generated carrier's double-shift formula.
fn bit_at_a_time_model(
    operation: Operation,
    bits: u32,
    destination: u32,
    source: u32,
    count: u8,
) -> ModelResult {
    let count = u32::from(count & 31);
    // Counts beyond a word are architecturally undefined. The emulator's
    // deterministic policy clears the result, CF, AF and OF.
    if count > bits {
        return ModelResult {
            value: 0,
            status: Some(Flags {
                cf: 0,
                pf: 1,
                af: 0,
                zf: 1,
                sf: 0,
                of: 0,
            }),
        };
    }
    let modulus = 2_u64.pow(bits);
    let sign = modulus / 2;
    let original = u64::from(destination) % modulus;
    let source = u64::from(source) % modulus;
    let mut result = original;
    let mut carry = false;
    for index in 0..count {
        match operation {
            Operation::Shld => {
                carry = result >= sign;
                let incoming = source / 2_u64.pow(bits - 1 - index) % 2;
                result = result * 2 % modulus + incoming;
            }
            Operation::Shrd => {
                carry = result % 2 != 0;
                let incoming = source / 2_u64.pow(index) % 2;
                result = result / 2 + incoming * sign;
            }
        }
    }
    ModelResult {
        value: result as u32,
        status: (count != 0).then_some(Flags {
            cf: u8::from(carry),
            pf: u8::from((result as u8).count_ones() % 2 == 0),
            af: 0,
            zf: u8::from(result == 0),
            sf: u8::from(result >= sign),
            of: u8::from(count == 1 && (original >= sign) != (result >= sign)),
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
