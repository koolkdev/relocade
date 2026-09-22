//! Stack offsets refer to ST(i) before any push or pop performed by the operation.

use super::*;
use crate::execution::x87::{
    exchange_register, free_register, load_extended, load_register, rotate_stack, store_extended,
    store_register,
};

instruction_families! {
    FLD_REGISTER {
        execute: load_register;
        forms { 0xD9 @ 0xC0 + rm => operands(st); }
    }
    FLD_EXTENDED {
        execute: load_extended;
        forms { 0xDB / 5 => operands(mem); }
    }
    FXCH {
        execute: exchange_register;
        forms { 0xD9 @ 0xC8 + rm => operands(st); }
    }
    FST_REGISTER {
        execute: store_register(false);
        forms { 0xDD @ 0xD0 + rm => operands(st); }
    }
    FSTP_REGISTER {
        execute: store_register(true);
        forms { 0xDD @ 0xD8 + rm => operands(st); }
    }
    FSTP_EXTENDED {
        execute: store_extended;
        forms { 0xDB / 7 => operands(mem); }
    }
    FFREE {
        execute: free_register;
        forms { 0xDD @ 0xC0 + rm => operands(st); }
    }
    FINCSTP {
        execute: rotate_stack(true);
        forms { 0xD9 @ 0xF7 => no_operands(); }
    }
    FDECSTP {
        execute: rotate_stack(false);
        forms { 0xD9 @ 0xF6 => no_operands(); }
    }
}
