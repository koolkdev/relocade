use wasm86_x86::{CpuState, StatusFlags, StoredFlags};

use crate::support::machine::{byte_register_image, Exit, Image, Step};

#[path = "rotates/counts.rs"]
mod counts;
#[path = "rotates/decoding.rs"]
mod decoding;
#[path = "rotates/flags.rs"]
mod flags;
#[path = "rotates/memory.rs"]
mod memory;
#[path = "rotates/publication.rs"]
mod publication;
#[path = "rotates/sequences.rs"]
mod sequences;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Operation {
    Rol,
    Ror,
}

impl Operation {
    fn extension(self) -> u8 {
        match self {
            Self::Rol => 0,
            Self::Ror => 1,
        }
    }
}

const OPERATIONS: [Operation; 2] = [Operation::Rol, Operation::Ror];

// SUB32 0x7ffffffe - 0xfffffffe produces 0x80000000.
const PRIOR_FLAGS: StatusFlags = StatusFlags {
    cf: 1,
    pf: 1,
    af: 0,
    zf: 0,
    sf: 1,
    of: 1,
};

const STORED_FLAGS: StoredFlags = StoredFlags {
    kind: 9,
    reserved: [0xa5; 3],
    left: 0x7fff_fffe,
    right: 0xffff_fffe,
    status: StatusFlags {
        cf: 0xfe,
        pf: 0x7f,
        af: 0x5a,
        zf: 0x80,
        sf: 0xff,
        of: 1,
    },
    non_status: [0, 1, 0, 0, 0, 0xa5],
};

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags = STORED_FLAGS;
    image
}

struct ModelResult {
    value: u32,
    status: Option<StatusFlags>,
}

impl ModelResult {
    fn apply_flags(&self, cpu: &mut CpuState) {
        if let Some(status) = self.status {
            cpu.flags.kind = 0;
            cpu.flags.status = status;
        }
    }
}

// Rotate one bit at a time with widened arithmetic. The masked x86 count
// controls flag changes even when complete turns restore the original value.
fn bit_at_a_time_model(
    operation: Operation,
    bits: u32,
    value: u32,
    count: u8,
    prior: StatusFlags,
) -> ModelResult {
    let modulus = 2_u64.pow(bits);
    let sign = modulus / 2;
    let mut result = u64::from(value) % modulus;
    let count = count & 31;
    let mut carry = false;
    for _ in 0..count {
        match operation {
            Operation::Rol => {
                carry = result >= sign;
                result = result * 2 % modulus + u64::from(carry);
            }
            Operation::Ror => {
                carry = result % 2 != 0;
                result = result / 2 + if carry { sign } else { 0 };
            }
        }
    }
    let overflow = count == 1
        && match operation {
            Operation::Rol => (result >= sign) != carry,
            Operation::Ror => (result >= sign) != (result % sign >= sign / 2),
        };
    ModelResult {
        value: result as u32,
        status: (count != 0).then_some(StatusFlags {
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
