use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Esp};

use crate::support::cases::{
    FlagExpectation::{Clear, Preserved, Set},
    Flags, InstructionCase as Case,
};

const INITIAL: Flags<bool> = Flags {
    cf: true,
    pf: true,
    af: false,
    zf: false,
    sf: true,
    of: true,
};

// OF is the project's deterministic zero policy when the masked count exceeds one.
#[rustfmt::skip]
pub(super) fn implicit_one_cases() -> Vec<Case> {
    vec![
        Case::new("ROL AL,1; input 81", &[0xd0, 0xc0], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_2203),
        Case::new("ROL AX,1; input 8001", &[0x66, 0xd1, 0xc0], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_0003),
        Case::new("ROL EAX,1; input 80000001", &[0xd1, 0xc0], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0x0000_0003),
        Case::new("ROR AL,1; input 81", &[0xd0, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2281, 0x4433_22c0),
        Case::new("ROR AX,1; input 8001", &[0x66, 0xd1, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8001, 0x4433_c000),
        Case::new("ROR EAX,1; input 80000001", &[0xd1, 0xc8], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x8000_0001, 0xc000_0000),
        Case::new("ROL AL,1; input 40", &[0xd0, 0xc0], INITIAL,
            Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2240, 0x4433_2280),
        Case::new("ROL AX,1; input 4000", &[0x66, 0xd1, 0xc0], INITIAL,
            Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_4000, 0x4433_8000),
        Case::new("ROL EAX,1; input 40000000", &[0xd1, 0xc0], INITIAL,
            Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4000_0000, 0x8000_0000),
        Case::new("ROR AL,1; input 1", &[0xd0, 0xc8], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_2201, 0x4433_2280),
        Case::new("ROR AX,1; input 1", &[0x66, 0xd1, 0xc8], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_0001, 0x4433_8000),
        Case::new("ROR EAX,1; input 1", &[0xd1, 0xc8], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0001, 0x8000_0000),
    ]
}

#[rustfmt::skip]
pub(super) fn alias_cases() -> Vec<Case> {
    vec![
        Case::new("ROL CL,CL reads the old count", &[0xd2, 0xc1], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_6603, 0x8877_6618),
        Case::new("ROR CH,CL preserves CL", &[0xd2, 0xcd], INITIAL,
            Flags { cf: Clear, of: Set, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_8001, 0x8877_4001),
        Case::new("ROR CX,CL reads the old word", &[0x66, 0xd3, 0xc9], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Ecx, 0x8877_8001, 0x8877_c000),
        Case::new("ROL ECX,CL reads both old views", &[0xd3, 0xc1], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Ecx, 0x8000_0001, 0x0000_0003),
        Case::new("ROR AH,CL preserves AL", &[0xd2, 0xcc], INITIAL,
            Flags { cf: Set, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x4433_8111, 0x4433_c011)
            .initial_register(Ecx, 0x8877_6601),
        Case::new("ROL BH,1 ignores 66", &[0x66, 0xd0, 0xc7], INITIAL,
            Flags { cf: Set, of: Set, ..Flags::all(Preserved) })
            .register(Ebx, 0x10ff_81dd, 0x10ff_03dd)
            .initial_register(Ecx, 0x8877_6601),
        Case::new("ROR SP,4 preserves upper ESP", &[0x66, 0xc1, 0xcc, 0x04], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Esp, 0x8765_8010, 0x8765_0801)
            .initial_register(Ecx, 0x8877_6604),
    ]
}

#[rustfmt::skip]
pub(super) fn full_turn_cases() -> Vec<Case> {
    vec![
        Case::new("ROL AL,8", &[0xc0, 0xc0, 0x08], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0080, 0x0000_0080),
        Case::new("ROL AL,16", &[0xc0, 0xc0, 0x10], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0080, 0x0000_0080),
        Case::new("ROL AL,24", &[0xc0, 0xc0, 0x18], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0080, 0x0000_0080),
        Case::new("ROR AL,8", &[0xc0, 0xc8, 0x08], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0001, 0x0000_0001),
        Case::new("ROR AL,16", &[0xc0, 0xc8, 0x10], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0001, 0x0000_0001),
        Case::new("ROR AL,24", &[0xc0, 0xc8, 0x18], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0001, 0x0000_0001),
        Case::new("ROL AX,16", &[0x66, 0xc1, 0xc0, 0x10], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_8000, 0x0000_8000),
        Case::new("ROR AX,16", &[0x66, 0xc1, 0xc8, 0x10], INITIAL,
            Flags { cf: Clear, of: Clear, ..Flags::all(Preserved) })
            .register(Eax, 0x0000_0001, 0x0000_0001),
    ]
}
