//! Register multiplication reads ST(i) before an optional pop.

use super::*;
use crate::execution::x87::{multiply_register, ProductDestination};

instruction_families! {
    FMUL_TOP {
        execute: multiply_register(ProductDestination::Top, false);
        forms { 0xD8 @ 0xC8 + rm => operands(st); }
    }
    FMUL_REGISTER {
        execute: multiply_register(ProductDestination::Other, false);
        forms { 0xDC @ 0xC8 + rm => operands(st); }
    }
    FMULP_REGISTER {
        execute: multiply_register(ProductDestination::Other, true);
        forms { 0xDE @ 0xC8 + rm => operands(st); }
    }
}
