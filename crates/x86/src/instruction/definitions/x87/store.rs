//! The opcode fixes the destination format independently of operand size.

use super::*;
use crate::{
    execution::x87::{store_binary, store_integer},
    x87::BinaryFormat,
};

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
    FIST {
        execute: store_integer::<_>(false);
        forms {
            0xDF / 2 => word(mem);
            0xDB / 2 => dword(mem);
        }
    }
    FISTP {
        execute: store_integer::<_>(true);
        forms {
            0xDF / 3 => word(mem);
            0xDB / 3 => dword(mem);
            0xDF / 7 => qword(mem);
        }
    }
}
