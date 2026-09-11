use crate::support::{arithmetic, machine::Image};

#[path = "bit_scans/cases.rs"]
mod cases;
#[path = "bit_scans/decoding.rs"]
mod decoding;
#[path = "bit_scans/memory.rs"]
mod memory;
#[path = "bit_scans/model.rs"]
mod model;
#[path = "bit_scans/publication.rs"]
mod publication;
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

use crate::support::cases::{
    FlagExpectation,
    FlagExpectation::{Clear, Set},
    Flags,
};

const ZERO: Flags<FlagExpectation> = Flags {
    cf: Clear,
    pf: Set,
    af: Clear,
    zf: Set,
    sf: Clear,
    of: Clear,
};
const EVEN: Flags<FlagExpectation> = Flags { zf: Clear, ..ZERO };
const ODD: Flags<FlagExpectation> = Flags { pf: Clear, ..EVEN };
