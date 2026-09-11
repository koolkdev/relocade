use wasm86_x86::Gpr32::{Eax, Edx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};

// Intel SDM 325462-089, Volume 2, BSF: zero preserves the destination,
// sets ZF/PF, and clears CF/AF/SF/OF. These guarantees are explicit in this revision.
#[rustfmt::skip]
fn zero_source_cases() -> Vec<Case> {
    let initial = Flags {
        cf: true,
        pf: false,
        af: true,
        zf: false,
        sf: true,
        of: true,
    };
    let expected = Flags {
        cf: Clear,
        pf: Set,
        af: Clear,
        zf: Set,
        sf: Clear,
        of: Clear,
    };
    vec![
        Case::new("BSF AX,DX; source=0", &[0x66, 0x0f, 0xbc, 0xc2], initial, expected)
            .at(0x1ffe)
            .register(Eax, 0x4433_2281, 0x4433_2281)
            .initial_register(Edx, 0xdead_0000),
        Case::new("BSF EAX,EDX; source=0", &[0x0f, 0xbc, 0xc2], initial, expected)
            .register(Eax, 0x4433_2281, 0x4433_2281)
            .initial_register(Edx, 0),
    ]
}

test_cases!(
    zero_source_preserves_destination_and_sets_defined_flags,
    zero_source_cases()
);
