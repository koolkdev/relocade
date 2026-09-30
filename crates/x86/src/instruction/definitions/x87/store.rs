//! The opcode fixes the destination format independently of operand size.

use super::*;
use crate::{execution::x87::store_binary, x87::BinaryFormat};

instruction_families! {
    FST_BINARY32 {
        execute: store_binary(BinaryFormat::Binary32, false);
        forms { 0xD9 / 2 => operands(mem); }
    }
    FSTP_BINARY32 {
        execute: store_binary(BinaryFormat::Binary32, true);
        forms { 0xD9 / 3 => operands(mem); }
    }
    FST_BINARY64 {
        execute: store_binary(BinaryFormat::Binary64, false);
        forms { 0xDD / 2 => operands(mem); }
    }
    FSTP_BINARY64 {
        execute: store_binary(BinaryFormat::Binary64, true);
        forms { 0xDD / 3 => operands(mem); }
    }
}
