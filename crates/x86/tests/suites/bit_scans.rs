use wasm86_x86::{CpuState, Gpr32, StatusFlags};

use crate::support::{
    arithmetic,
    machine::{Exit, Image, Step},
};

#[path = "bit_scans/decoding.rs"]
mod decoding;
#[path = "bit_scans/memory.rs"]
mod memory;
#[path = "bit_scans/registers.rs"]
mod registers;
#[path = "bit_scans/sequences.rs"]
mod sequences;

#[derive(Clone, Copy, Debug)]
enum Operation {
    Bsf,
    Bsr,
}

impl Operation {
    fn opcode(self) -> u8 {
        match self {
            Self::Bsf => 0xbc,
            Self::Bsr => 0xbd,
        }
    }
}

const OPERATIONS: [Operation; 2] = [Operation::Bsf, Operation::Bsr];

fn image(code: &[u8]) -> Image {
    let mut image = arithmetic::image(code);
    image.cpu.registers.eax = 0x4433_a55b;
    image.cpu.flags.left = 0x1234_5678;
    image.cpu.flags.right = 0x8765_4321;
    image
}

struct Expected {
    destination: u32,
    flags: StatusFlags,
}

impl Expected {
    fn apply(&self, cpu: &mut CpuState, destination: Gpr32) {
        cpu.registers[destination] = self.destination;
        cpu.flags.kind = 0;
        cpu.flags.status = self.flags;
    }
}

// Enumerate bit positions with integer division, independently of clz/ctz.
// Scan parity covers every bit of the logical source operand.
fn expected(operation: Operation, bits: u32, source: u32, previous: u32) -> Expected {
    let source = u64::from(source) % 2_u64.pow(bits);
    let mut positions = (0..bits).filter(|bit| source / 2_u64.pow(*bit) % 2 != 0);
    let index = match operation {
        Operation::Bsf => positions.next(),
        Operation::Bsr => positions.next_back(),
    };
    let destination = match index {
        None => previous,
        Some(index) if bits == 16 => (previous & 0xffff_0000) | index,
        Some(index) => index,
    };
    let odd_bits = (0..bits)
        .map(|bit| source / 2_u64.pow(bit) % 2)
        .sum::<u64>()
        % 2;
    Expected {
        destination,
        flags: StatusFlags {
            cf: 0,
            pf: (1 - odd_bits) as u8,
            af: 0,
            zf: u8::from(index.is_none()),
            sf: 0,
            of: 0,
        },
    }
}

fn retire(cpu: &mut CpuState, length: u32) -> Step<'static> {
    cpu.eip = cpu.eip.wrapping_add(length);
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    Step {
        cpu: *cpu,
        ram: &[],
        exit: Exit::Dispatch(cpu.eip),
    }
}
