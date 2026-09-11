use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Esp};

use crate::support::cases::{
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};

// AF is the project's deterministic zero policy; OF is zero when the masked count exceeds one.
#[rustfmt::skip]
pub(super) fn implicit_one_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHL AL,1; input 81", &[0xd0, 0xe0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_2281, 0x4433_2202),
        Case::replacing_flags("SHL AX,1; input 8001", &[0x66, 0xd1, 0xe0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_8001, 0x4433_0002),
        Case::replacing_flags("SHL EAX,1; input 80000001", &[0xd1, 0xe0],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0001, 0x0000_0002),
        Case::replacing_flags("SHR AL,1; input 81", &[0xd0, 0xe8],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_2281, 0x4433_2240),
        Case::replacing_flags("SHR AX,1; input 8001", &[0x66, 0xd1, 0xe8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x4433_8001, 0x4433_4000),
        Case::replacing_flags("SHR EAX,1; input 80000001", &[0xd1, 0xe8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_0001, 0x4000_0000),
        Case::replacing_flags("SAR AL,1; input 81", &[0xd0, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_2281, 0x4433_22c0),
        Case::replacing_flags("SAR AX,1; input 8001", &[0x66, 0xd1, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8001, 0x4433_c000),
        Case::replacing_flags("SAR EAX,1; input 80000001", &[0xd1, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_0001, 0xc000_0000),
        Case::replacing_flags("SAR AL,1; input 7f", &[0xd0, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_227f, 0x4433_223f),
        Case::replacing_flags("SAR AX,1; input 7fff", &[0x66, 0xd1, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_7fff, 0x4433_3fff),
        Case::replacing_flags("SAR EAX,1; input 7fffffff", &[0xd1, 0xf8],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x7fff_ffff, 0x3fff_ffff),
    ]
}

#[rustfmt::skip]
pub(super) fn alias_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("SHL CL,CL reads the old count", &[0xd2, 0xe1],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Ecx, 0x8877_6603, 0x8877_6618),
        Case::replacing_flags("SHR CH,CL preserves CL", &[0xd2, 0xed],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Ecx, 0x8877_8001, 0x8877_4001),
        Case::replacing_flags("SAR CX,CL reads the old word", &[0x66, 0xd3, 0xf9],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Ecx, 0x8877_8001, 0x8877_c000),
        Case::replacing_flags("SHL ECX,CL reads both old views", &[0xd3, 0xe1],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Ecx, 0x8000_0001, 0x0000_0002),
        Case::replacing_flags("SAR AH,CL preserves AL", &[0xd2, 0xfc],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_8111, 0x4433_c011)
            .initial_register(Ecx, 0x8877_6601),
        Case::replacing_flags("SHL BH,1 ignores 66", &[0x66, 0xd0, 0xe7],
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Ebx, 0x10ff_81dd, 0x10ff_02dd)
            .initial_register(Ecx, 0x8877_6601),
        Case::replacing_flags("SHR SP,4 preserves upper ESP", &[0x66, 0xc1, 0xec, 0x04],
            Flags { cf: Clear, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Esp, 0x8765_8010, 0x8765_0801)
            .initial_register(Ecx, 0x8877_6604),
    ]
}

#[rustfmt::skip]
pub(super) fn zero_count_records() -> Vec<Case> {
    let concrete = wasm86_x86::StoredFlags { kind: 0, ..super::super::STORED_FLAGS };
    vec![
        Case::preserving_flags("SHL ECX,CL preserves concrete backing", &[0xd3, 0xe1])
            .stored_flags(concrete).initial_register(Ecx, 0x8877_6620),
        Case::preserving_flags("SHL AH,32 preserves concrete backing", &[0xc0, 0xe4, 0x20])
            .stored_flags(concrete).initial_register(Ecx, 0x8877_6620),
        Case::preserving_flags("SHL ECX,CL preserves lazy SUB backing", &[0xd3, 0xe1])
            .stored_flags(super::super::STORED_FLAGS).initial_register(Ecx, 0x8877_6620),
        Case::preserving_flags("SHL AH,32 preserves lazy SUB backing", &[0xc0, 0xe4, 0x20])
            .stored_flags(super::super::STORED_FLAGS).initial_register(Ecx, 0x8877_6620),
    ]
}
