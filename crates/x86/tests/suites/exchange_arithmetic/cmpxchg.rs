use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::ReadWrite,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

#[rustfmt::skip]
fn registers() -> Vec<Case> {
    vec![
        Case::new("byte mismatch replaces AL with old CL", &[0x0f, 0xb0, 0xd1], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_2255).initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99),
        Case::new("byte match replaces CL with DL", &[0x0f, 0xb0, 0xd1], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x8877_6611, 0x8877_6699).initial_register(Edx, 0xccbb_aa99),
        Case::new("word mismatch preserves the upper accumulator half", &[0x66, 0x0f, 0xb1, 0xd1], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_6655).initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99),
        Case::new("word match preserves the upper destination half", &[0x66, 0x0f, 0xb1, 0xd1], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x8877_2211, 0x8877_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::new("dword mismatch replaces EAX with old ECX", &[0x0f, 0xb1, 0xd1], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_2211, 0x8877_6655).initial_register(Ecx, 0x8877_6655).initial_register(Edx, 0xccbb_aa99),
        Case::new("dword match replaces ECX with EDX", &[0x0f, 0xb1, 0xd1], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x4433_2211, 0xccbb_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::new("AL destination takes DL", &[0x0f, 0xb0, 0xd0], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_2299).initial_register(Edx, 0xccbb_aa99),
        Case::new("AX destination takes DX", &[0x66, 0x0f, 0xb1, 0xd0], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::new("EAX destination takes EDX", &[0x0f, 0xb1, 0xd0], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0xccbb_aa99).initial_register(Edx, 0xccbb_aa99),
        Case::new("AH mismatch replaces AL without replacing AH", &[0x0f, 0xb0, 0xcc], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_2222).initial_register(Ecx, 0x8877_6655),
        Case::new("AH match changes AH while preserving AL", &[0x0f, 0xb0, 0xcc], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_1111, 0x4433_5511).initial_register(Ecx, 0x8877_6655),
        Case::new("AH source supplies its old value to matching CL", &[0x0f, 0xb0, 0xe1], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).register(Ecx, 0x8877_6611, 0x8877_6622),
        Case::new("an accumulator source does not overwrite a mismatch destination", &[0x0f, 0xb1, 0xc1], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_2211, 0x8877_6655).initial_register(Ecx, 0x8877_6655),
        Case::new("a matching source and destination still publish the comparison", &[0x0f, 0xb1, 0xc9], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).initial_register(Ecx, 0x4433_2211),
    ]
}

test_cases!(register_outcomes_and_aliases, registers());

#[rustfmt::skip]
fn comparison_conditions() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new("equal dword comparison", Flags::all(true))
            .initial_register(Eax, 0x4433_2211).initial_register(Ecx, 0x4433_2211).initial_register(Edx, 0xccbb_aa99)
            .step(Checkpoint::new(&[0x0f, 0xb1, 0xd1],
                Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
                .register(Ecx, 0xccbb_aa99))
            .conditions([0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
        SequenceCase::new("byte comparison overflows with an unsigned borrow", Flags::all(true))
            .initial_register(Eax, 0x4433_227f).initial_register(Ecx, 0x8877_66ff).initial_register(Edx, 0xccbb_aa99)
            .step(Checkpoint::new(&[0x0f, 0xb0, 0xd1],
                Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x4433_22ff))
            .conditions([1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1, 0, 1]),
    ]
}

test_sequences!(setcc_consumers, comparison_conditions());

#[rustfmt::skip]
fn memory_addresses() -> Vec<Case> {
    vec![
        Case::new("word mismatch keeps the old EAX address and upper half", &[0x66, 0x0f, 0xb1, 0x10], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x1111_4020, 0x1111_fedc).initial_register(Edx, 0xccbb_aa99)
            .memory(0x1111_4020, &[0xdc, 0xfe], ReadWrite),
        Case::new("matching memory destination takes its EBX address source", &[0x0f, 0xb1, 0x1b], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).initial_register(Ebx, 0x4020)
            .memory(0x4020, &[0x11, 0x22, 0x33, 0x44], ReadWrite).expect_memory(0x4020, &[0x20, 0x40, 0, 0]),
        Case::new("matching memory destination takes its scaled ECX index source", &[0x0f, 0xb1, 0x4c, 0x8b, 0x20], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Set, sf: Clear, of: Clear })
            .initial_register(Eax, 0x4433_2211).initial_register(Ebx, 0x4000).initial_register(Ecx, 8)
            .memory(0x4040, &[0x11, 0x22, 0x33, 0x44], ReadWrite).expect_memory(0x4040, &[8, 0, 0, 0]),
    ]
}

test_cases!(address_aliases, memory_addresses());
