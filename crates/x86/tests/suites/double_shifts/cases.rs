use wasm86_x86::Gpr32::{Eax, Ebx, Edx};

use crate::support::cases::{
    test_cases, FlagExpectation::Undefined, Flags, InstructionCase as Case, Permissions::ReadWrite,
    RegisterExpectation::DefinedBits,
};

// Intel SDM 325462-089, Volume 2, SHLD: counts above the word width
// leave the result and status flags undefined. Other storage is unaffected.
#[rustfmt::skip]
fn undefined_word_results() -> Vec<Case> {
    let initial = Flags {
        cf: true,
        pf: false,
        af: true,
        zf: false,
        sf: true,
        of: false,
    };
    let expected = Flags {
        cf: Undefined,
        pf: Undefined,
        af: Undefined,
        zf: Undefined,
        sf: Undefined,
        of: Undefined,
    };
    vec![
        Case::new("SHLD AX,DX,17", &[0x66, 0x0f, 0xa4, 0xd0, 0x11], initial, expected)
            .initial_register(Eax, 0x4433_2281)
            .initial_register(Edx, 0x1234_0001)
            .expect_register(Eax, DefinedBits { value: 0x4433_0000, mask: 0xffff_0000 }),
        Case::new("SHLD word [EBX],AX,17", &[0x66, 0x0f, 0xa4, 0x03, 0x11], initial, expected)
            .initial_register(Eax, 0x1234_0001)
            .initial_register(Ebx, 0x4fff)
            .memory(0x4ffe, &[0xa5, 0x81, 0x80, 0x5a], ReadWrite)
            .undefined_memory(0x4fff, 2),
    ]
}

test_cases!(
    undefined_word_results_preserve_other_storage,
    undefined_word_results()
);
