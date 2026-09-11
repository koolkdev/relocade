use crate::support::cases::{FlagExpectation::Undefined, Flags, InstructionCase};

#[path = "division/arithmetic.rs"]
mod arithmetic;
#[path = "division/decoding.rs"]
mod decoding;
#[path = "division/errors.rs"]
mod errors;
#[path = "division/memory.rs"]
mod memory;
#[path = "division/register_sources.rs"]
mod register_sources;
#[path = "division/sequences.rs"]
mod sequences;

// Successful DIV/IDIV leave all six status flags architecturally undefined.
// Start with a valid logical record so a policy that preserves it is also valid.
fn successful_division(name: impl Into<String>, code: &[u8]) -> InstructionCase {
    InstructionCase::new(
        name,
        code,
        Flags {
            cf: true,
            pf: false,
            af: true,
            zf: true,
            sf: false,
            of: false,
        },
        Flags::all(Undefined),
    )
}
