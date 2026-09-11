use crate::support::cases::{FlagExpectation, Flags};

#[path = "multiply/decoding.rs"]
mod decoding;
#[path = "multiply/explicit.rs"]
mod explicit;
#[path = "multiply/implicit.rs"]
mod implicit;
#[path = "multiply/memory.rs"]
mod memory;
#[path = "multiply/sequences.rs"]
mod sequences;

// CF and OF both report whether the product fits the low destination width.
// PF, AF, ZF and SF are architecturally undefined after MUL and IMUL.
fn product_flags(overflow: FlagExpectation) -> Flags<FlagExpectation> {
    Flags {
        cf: overflow,
        pf: FlagExpectation::Undefined,
        af: FlagExpectation::Undefined,
        zf: FlagExpectation::Undefined,
        sf: FlagExpectation::Undefined,
        of: overflow,
    }
}
