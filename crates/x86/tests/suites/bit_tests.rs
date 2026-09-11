use wasm86_x86::{CpuState, StatusFlags};

use crate::support::machine::{byte_register_image, Exit, Image, Step};

#[path = "bit_tests/decoding.rs"]
mod decoding;
#[path = "bit_tests/faults.rs"]
mod faults;
#[path = "bit_tests/flags.rs"]
mod flags;
#[path = "bit_tests/memory.rs"]
mod memory;
#[path = "bit_tests/registers.rs"]
mod registers;
#[path = "bit_tests/sequences.rs"]
mod sequences;

#[derive(Clone, Copy, Debug)]
enum Operation {
    Bt,
    Bts,
    Btr,
    Btc,
}

impl Operation {
    fn register_opcode(self) -> u8 {
        match self {
            Self::Bt => 0xa3,
            Self::Bts => 0xab,
            Self::Btr => 0xb3,
            Self::Btc => 0xbb,
        }
    }

    fn extension(self) -> u8 {
        match self {
            Self::Bt => 4,
            Self::Bts => 5,
            Self::Btr => 6,
            Self::Btc => 7,
        }
    }

    fn modifies(self) -> bool {
        !matches!(self, Self::Bt)
    }
}

const OPERATIONS: [Operation; 4] = [
    Operation::Bt,
    Operation::Bts,
    Operation::Btr,
    Operation::Btc,
];

fn prior_flags() -> StatusFlags {
    StatusFlags {
        cf: 0,
        pf: 0,
        af: 1,
        zf: 1,
        sf: 0,
        of: 1,
    }
}

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags.kind = 0;
    image.cpu.flags.left = 0x1234_5678;
    image.cpu.flags.right = 0x8765_4321;
    image.cpu.flags.status = StatusFlags {
        cf: 0xfe,
        pf: 0xfe,
        af: 0xff,
        zf: 0x7f,
        sf: 0x80,
        of: 0x5b,
    };
    image.cpu.flags.non_status = [0, 1, 0, 0, 0, 0xa5];
    image
}

struct Expected {
    value: u32,
    carry: u8,
}

impl Expected {
    fn apply_flags(&self, cpu: &mut CpuState, prior: StatusFlags) {
        cpu.flags.kind = 0;
        cpu.flags.status = StatusFlags {
            cf: self.carry,
            ..prior
        };
    }
}

// Determine the old bit by division, then add or subtract its place value.
// This oracle does not reuse the generated bit masks and Boolean operations.
fn expected(operation: Operation, bits: u32, value: u32, index: u32) -> Expected {
    let value = u64::from(value) % 2_u64.pow(bits);
    let place = 2_u64.pow(index % bits);
    let carry = value / place % 2;
    let result = match operation {
        Operation::Bt => value,
        Operation::Bts => value + (1 - carry) * place,
        Operation::Btr => value - carry * place,
        Operation::Btc if carry == 0 => value + place,
        Operation::Btc => value - place,
    };
    Expected {
        value: result as u32,
        carry: carry as u8,
    }
}

// Euclidean division chooses the containing operand for negative bit indexes.
// Immediate indexes always stay in the operand at the encoded address.
fn operand_address(base: u32, bits: u32, index: u32, immediate: bool) -> u32 {
    if immediate {
        return base;
    }
    let index = if bits == 16 {
        i64::from(index as i16)
    } else {
        i64::from(index as i32)
    };
    let units = index.div_euclid(i64::from(bits));
    base.wrapping_add((units * i64::from(bits / 8)) as u32)
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
