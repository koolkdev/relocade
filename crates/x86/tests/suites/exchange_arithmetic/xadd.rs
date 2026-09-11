use wasm86_x86::Gpr32::{Eax, Ebp, Ebx, Ecx, Edi, Esi, Esp};

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
fn width_conditions() -> Vec<SequenceCase> {
    vec![
        SequenceCase::new("byte carry and signed overflow", Flags::all(true))
            .initial_register(Eax, 0x4433_2280).initial_register(Ebx, 0x10ff_ee80)
            .step(Checkpoint::new(&[0x0f, 0xc0, 0xd8],
                Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
                .register(Eax, 0x4433_2200))
            .conditions([1, 0, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 1, 0, 1, 0]),
        SequenceCase::new("byte auxiliary carry with odd low-byte parity", Flags::all(true))
            .initial_register(Eax, 0x4433_220f).initial_register(Ebx, 0x10ff_ee01)
            .step(Checkpoint::new(&[0x0f, 0xc0, 0xd8],
                Flags { cf: Clear, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
                .register(Eax, 0x4433_2210).register(Ebx, 0x10ff_ee0f))
            .conditions([0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1, 0, 1]),
        SequenceCase::new("word signed overflow with even low-byte parity", Flags::all(true))
            .initial_register(Eax, 0x4433_7fff).initial_register(Ebx, 0x10ff_0001)
            .step(Checkpoint::new(&[0x66, 0x0f, 0xc1, 0xd8],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
                .register(Eax, 0x4433_8000).register(Ebx, 0x10ff_7fff))
            .conditions([1, 0, 0, 1, 0, 1, 0, 1, 1, 0, 1, 0, 0, 1, 0, 1]),
        SequenceCase::new("dword carry and zero without signed overflow", Flags::all(true))
            .initial_register(Eax, 0xffff_ffff).initial_register(Ebx, 1)
            .step(Checkpoint::new(&[0x0f, 0xc1, 0xd8],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
                .register(Eax, 0).register(Ebx, 0xffff_ffff))
            .conditions([0, 1, 1, 0, 1, 0, 1, 0, 0, 1, 1, 0, 0, 1, 1, 0]),
    ]
}

test_sequences!(exchanged_results_and_conditions, width_conditions());

#[rustfmt::skip]
fn aliases() -> Vec<Case> {
    vec![
        Case::new("XADD AL,AH ignores the operand-size prefix", &[0x66, 0x0f, 0xc0, 0xe0], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_1133),
        Case::new("XADD AH,AL reads both old aliases", &[0x0f, 0xc0, 0xc4], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_3322),
        Case::new("XADD AH,AH keeps the destination sum", &[0x0f, 0xc0, 0xe4], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_4411),
        Case::new("XADD AX,AX keeps its upper half", &[0x66, 0x0f, 0xc1, 0xc0], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2211, 0x4433_4422),
        Case::new("XADD EAX,EAX carries and overflows", &[0x0f, 0xc1, 0xc0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0),
        Case::new("XADD ESI,EBP returns old ESI to EBP", &[0x0f, 0xc1, 0xee], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Esi, 0x7777_7777, 0xdddd_dddd)
            .register(Ebp, 0x6666_6666, 0x7777_7777),
        Case::new("XADD SP,DI keeps both upper halves", &[0x66, 0x0f, 0xc1, 0xfc], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Esp, 0x5555_5555, 0x5555_dddd)
            .register(Edi, 0x8888_8888, 0x8888_5555),
    ]
}

#[rustfmt::skip]
fn memory_addresses() -> Vec<Case> {
    vec![
        Case::new("XADD [EAX],EAX keeps the old address base", &[0x0f, 0xc1, 0x00], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4010, 1).initial_register(Ebx, 0x3ff0)
            .memory(0x400f, &[0x5a, 1, 0, 0, 0, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[0x11, 0x40, 0, 0]),
        Case::new("XADD [EAX],AH keeps the old address parent", &[0x0f, 0xc0, 0x20], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4010, 0x8010).initial_register(Ebx, 0x3ff0)
            .memory(0x400f, &[0x5a, 0x80, 0x5a, 0x5a, 0x5a, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[0xc0]),
        Case::new("XADD word [EBX+ECX*4+16],CX uses the old scaled index", &[0x66, 0x0f, 0xc1, 0x4c, 0x8b, 0x10], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8000_0004, 0x8000_ffff).initial_register(Ebx, 0x3ff0)
            .memory(0x400f, &[0x5a, 0xff, 0xff, 0x5a, 0x5a, 0x5a], ReadWrite)
            .expect_memory(0x4010, &[3, 0]),
    ]
}

test_cases!(register_aliases, aliases());
test_cases!(address_aliases, memory_addresses());
