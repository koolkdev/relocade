//! x87 control words have fixed widths, independent of operand-size prefixes.

use super::*;
use crate::execution::x87::{
    clear_exceptions, initialize, load_control, store_control, store_status, wait,
};

instruction_families! {
    FNINIT {
        execute: initialize;
        forms { 0xDB @ 0xE3 => no_operands(); }
    }
    FNCLEX {
        execute: clear_exceptions;
        forms { 0xDB @ 0xE2 => no_operands(); }
    }
    FWAIT {
        execute: wait;
        forms { 0x9B => no_operands(); }
    }
    FLDCW {
        execute: load_control;
        forms { 0xD9 / 5 => word(mem16); }
    }
    FNSTCW {
        execute: store_control;
        forms { 0xD9 / 7 => word(mem16); }
    }
    FNSTSW {
        execute: store_status;
        forms {
            0xDD / 7 => word(mem16);
            0xDF @ 0xE0 => word(AX);
        }
    }
}
