//! The opcode fixes the source format; the operand-size prefix does not change it.

use super::*;
use crate::{execution::x87::load_binary, state::BinaryFormat};

instruction_families! {
    FLD_BINARY32 {
        execute: load_binary(BinaryFormat::Binary32);
        forms { 0xD9 / 0 => operands(mem); }
    }
    FLD_BINARY64 {
        execute: load_binary(BinaryFormat::Binary64);
        forms { 0xDD / 0 => operands(mem); }
    }
}
