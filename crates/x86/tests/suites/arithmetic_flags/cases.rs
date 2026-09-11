use wasm86_x86::Gpr32::{Eax, Ebx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};

// Intel SDM 325462-089, Volume 2, ADD instruction entry. These hand-reviewed
// literals check all six logical flags without depending on the stored recipe.
#[rustfmt::skip]
fn cases() -> Vec<Case> {
    vec![
        Case::new(
            "ADD AL,BL: signed overflow, upper EAX preserved", &[0x00, 0xd8],
            Flags { cf: true, pf: true, af: false, zf: true, sf: false, of: false },
            Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set },
        )
        .register(Eax, 0x4433_227f, 0x4433_2280)
        .initial_register(Ebx, 1),
        Case::new(
            "ADD EAX,EBX: carry and signed overflow", &[0x01, 0xd8],
            Flags { cf: false, pf: false, af: true, zf: false, sf: true, of: false },
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set },
        )
        .register(Eax, 0x8000_0000, 0)
        .initial_register(Ebx, 0x8000_0000),
    ]
}

test_cases!(add_flags, cases());
