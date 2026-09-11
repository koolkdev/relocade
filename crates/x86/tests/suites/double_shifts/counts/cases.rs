use wasm86_x86::Gpr32::{Eax, Ecx, Edi, Edx, Esp};

use crate::support::cases::{
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};

// AF is the project's deterministic zero policy; OF is zero for masked counts above one.
#[rustfmt::skip]
pub(super) fn literal_results() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD EAX,EDX,1; zero result", &[0x0f, 0xa4, 0xd0, 0x01],
            Flags { cf: Set, pf: Set, af: Clear, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0x0000_0000)
            .initial_register(Edx, 0x0000_0000),
        Case::replacing_flags("SHRD EAX,EDX,1; source sets sign", &[0x0f, 0xac, 0xd0, 0x01],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_0000, 0xc000_0000)
            .initial_register(Edx, 0x0000_0001),
        Case::replacing_flags("SHRD EAX,EDX,1; source clears sign", &[0x0f, 0xac, 0xd0, 0x01],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0000, 0x4000_0000)
            .initial_register(Edx, 0x0000_0000),
        Case::replacing_flags("SHLD AX,DX,1; sign changes", &[0x66, 0x0f, 0xa4, 0xd0, 0x01],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_4000, 0x4433_8000)
            .initial_register(Edx, 0x0000_0000),
        Case::replacing_flags("SHLD AX,DX,16; source replaces word", &[0x66, 0x0f, 0xa4, 0xd0, 0x10],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_1234)
            .initial_register(Edx, 0x0000_1234),
        Case::replacing_flags("SHRD AX,DX,16; source replaces word", &[0x66, 0x0f, 0xac, 0xd0, 0x10],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8000, 0x4433_abcd)
            .initial_register(Edx, 0x0000_abcd),
    ]
}

#[rustfmt::skip]
pub(super) fn alias_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHLD 16-bit same destination and source", &[0x66, 0x0f, 0xa4, 0xc0, 0x10],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xccbb_a55a, 0xccbb_a55a)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHLD 32-bit same destination and source", &[0x0f, 0xa4, 0xc0, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xccbb_a55a, 0xa55a_ccbb)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHRD 16-bit same destination and source", &[0x66, 0x0f, 0xac, 0xc0, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xccbb_a55a, 0xccbb_a55a)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHRD 32-bit same destination and source", &[0x0f, 0xac, 0xc0, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0xccbb_a55a, 0xa55a_ccbb)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHLD 16-bit destination contains CL", &[0x66, 0x0f, 0xa5, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x8877_001d)
            .initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHLD 32-bit destination contains CL", &[0x0f, 0xa5, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x43bc_001e)
            .initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHRD 16-bit destination contains CL", &[0x66, 0x0f, 0xad, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x8877_5000)
            .initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHRD 32-bit destination contains CL", &[0x0f, 0xad, 0xd1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x510e_f000)
            .initial_register(Edx, 0xccbb_a55a),
        Case::replacing_flags("SHLD 16-bit source contains CL", &[0x66, 0x0f, 0xa5, 0xc8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_000c)
            .initial_register(Ecx, 0x8877_8003),
        Case::replacing_flags("SHLD 32-bit source contains CL", &[0x0f, 0xa5, 0xc8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x219c_000c)
            .initial_register(Ecx, 0x8877_8003),
        Case::replacing_flags("SHRD 16-bit source contains CL", &[0x66, 0x0f, 0xad, 0xc8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_7000)
            .initial_register(Ecx, 0x8877_8003),
        Case::replacing_flags("SHRD 32-bit source contains CL", &[0x0f, 0xad, 0xc8],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_8001, 0x6886_7000)
            .initial_register(Ecx, 0x8877_8003),
        Case::replacing_flags("SHLD 16-bit both operands contain CL", &[0x66, 0x0f, 0xa5, 0xc9],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x8877_001c),
        Case::replacing_flags("SHLD 32-bit both operands contain CL", &[0x0f, 0xa5, 0xc9],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x43bc_001c),
        Case::replacing_flags("SHRD 16-bit both operands contain CL", &[0x66, 0x0f, 0xad, 0xc9],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x8877_7000),
        Case::replacing_flags("SHRD 32-bit both operands contain CL", &[0x0f, 0xad, 0xc9],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_8003, 0x710e_f000),
        Case::replacing_flags("SHLD 16-bit SP needs no SIB", &[0x66, 0x0f, 0xa4, 0xfc, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Esp, 0x4433_8001, 0x4433_a55a)
            .initial_register(Edi, 0xccbb_a55a)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHLD 32-bit SP needs no SIB", &[0x0f, 0xa4, 0xfc, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Esp, 0x4433_8001, 0x8001_ccbb)
            .initial_register(Edi, 0xccbb_a55a)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHRD 16-bit SP needs no SIB", &[0x66, 0x0f, 0xac, 0xfc, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Esp, 0x4433_8001, 0x4433_a55a)
            .initial_register(Edi, 0xccbb_a55a)
            .initial_register(Ecx, 0x8877_8010),
        Case::replacing_flags("SHRD 32-bit SP needs no SIB", &[0x0f, 0xac, 0xfc, 0x10],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Esp, 0x4433_8001, 0xa55a_4433)
            .initial_register(Edi, 0xccbb_a55a)
            .initial_register(Ecx, 0x8877_8010),
    ]
}
