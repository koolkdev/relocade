use wasm86_x86::{Gpr32, StatusFlags, StoredFlags};

use crate::support::cases::{FlagExpectation, Flags};

use crate::support::machine::{byte_register_image, Image};

#[path = "bit_tests/decoding.rs"]
mod decoding;
#[path = "bit_tests/faults.rs"]
mod faults;
#[path = "bit_tests/flags.rs"]
mod flags;
#[path = "bit_tests/memory.rs"]
mod memory;
#[path = "bit_tests/model.rs"]
mod model;
#[path = "bit_tests/publication_sequence.rs"]
mod publication_sequence;
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

const INITIAL_REGISTERS: &[(Gpr32, u32)] = &[
    (Gpr32::Eax, 0x4433_2211),
    (Gpr32::Ecx, 0x8877_6655),
    (Gpr32::Edx, 0xccbb_aa99),
    (Gpr32::Ebx, 0x10ff_eedd),
];

fn other_register_inputs(operands: &[Gpr32]) -> Vec<(Gpr32, u32)> {
    INITIAL_REGISTERS
        .iter()
        .copied()
        .filter(|(register, _)| !operands.contains(register))
        .collect()
}

const INITIAL_FLAGS: Flags<bool> = Flags {
    cf: false,
    pf: false,
    af: true,
    zf: true,
    sf: false,
    of: true,
};

const STORED_FLAGS: StoredFlags = StoredFlags {
    kind: 0,
    reserved: [0xa5; 3],
    left: 0x1234_5678,
    right: 0x8765_4321,
    status: StatusFlags {
        cf: 0xfe,
        pf: 0xfe,
        af: 0xff,
        zf: 0x7f,
        sf: 0x80,
        of: 0x5b,
    },
    non_status: [0, 1, 0, 0, 0, 0xa5],
};

// All four instructions preserve ZF; Intel leaves PF, AF, SF and OF undefined.
fn bit_flags(carry: FlagExpectation) -> Flags<FlagExpectation> {
    use FlagExpectation::{Preserved, Undefined};
    Flags {
        cf: carry,
        pf: Undefined,
        af: Undefined,
        zf: Preserved,
        sf: Undefined,
        of: Undefined,
    }
}

fn image(code: &[u8]) -> Image {
    let mut image = byte_register_image(code);
    image.cpu.flags = STORED_FLAGS;
    image
}
