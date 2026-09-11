use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Esp};

use crate::support::cases::{
    FlagExpectation::{self, Clear, Preserved, Set},
    Flags, InstructionCase as Case,
};

const CARRY_CLEAR: Flags<bool> = Flags {
    cf: false,
    pf: false,
    af: true,
    zf: true,
    sf: false,
    of: true,
};
const CARRY_SET: Flags<bool> = Flags {
    cf: true,
    pf: false,
    af: true,
    zf: true,
    sf: false,
    of: true,
};

const CF_SET_OF_SET: Flags<FlagExpectation> = Flags {
    cf: Set,
    pf: Preserved,
    af: Preserved,
    zf: Preserved,
    sf: Preserved,
    of: Set,
};
const CF_SET_OF_CLEAR: Flags<FlagExpectation> = Flags {
    cf: Set,
    pf: Preserved,
    af: Preserved,
    zf: Preserved,
    sf: Preserved,
    of: Clear,
};
const CF_CLEAR_OF_SET: Flags<FlagExpectation> = Flags {
    cf: Clear,
    pf: Preserved,
    af: Preserved,
    zf: Preserved,
    sf: Preserved,
    of: Set,
};
const CF_CLEAR_OF_CLEAR: Flags<FlagExpectation> = Flags {
    cf: Clear,
    pf: Preserved,
    af: Preserved,
    zf: Preserved,
    sf: Preserved,
    of: Clear,
};
const FLAGS_PRESERVED: Flags<FlagExpectation> = Flags {
    cf: Preserved,
    pf: Preserved,
    af: Preserved,
    zf: Preserved,
    sf: Preserved,
    of: Preserved,
};

// Keep each literal case and its register expectations together.
#[rustfmt::skip]
pub(super) fn implicit_one_cases() -> Vec<Case> {
    vec![
        Case::new("RCL AL,1; CF=0", &[0xd0, 0xd0], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x4433_2281, 0x4433_2202),
        Case::new("RCL AX,1; CF=0", &[0x66, 0xd1, 0xd0], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x4433_8001, 0x4433_0002),
        Case::new("RCL EAX,1; CF=0", &[0xd1, 0xd0], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x8000_0001, 0x0000_0002),
        Case::new("RCL AL,1; CF=1", &[0xd0, 0xd0], CARRY_SET, CF_SET_OF_SET)
            .register(Eax, 0x4433_2281, 0x4433_2203),
        Case::new("RCL AX,1; CF=1", &[0x66, 0xd1, 0xd0], CARRY_SET, CF_SET_OF_SET)
            .register(Eax, 0x4433_8001, 0x4433_0003),
        Case::new("RCL EAX,1; CF=1", &[0xd1, 0xd0], CARRY_SET, CF_SET_OF_SET)
            .register(Eax, 0x8000_0001, 0x0000_0003),
        Case::new("RCR AL,1; CF=0", &[0xd0, 0xd8], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x4433_2281, 0x4433_2240),
        Case::new("RCR AX,1; CF=0", &[0x66, 0xd1, 0xd8], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x4433_8001, 0x4433_4000),
        Case::new("RCR EAX,1; CF=0", &[0xd1, 0xd8], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x8000_0001, 0x4000_0000),
        Case::new("RCR AL,1; CF=1", &[0xd0, 0xd8], CARRY_SET, CF_SET_OF_CLEAR)
            .register(Eax, 0x4433_2281, 0x4433_22c0),
        Case::new("RCR AX,1; CF=1", &[0x66, 0xd1, 0xd8], CARRY_SET, CF_SET_OF_CLEAR)
            .register(Eax, 0x4433_8001, 0x4433_c000),
        Case::new("RCR EAX,1; CF=1", &[0xd1, 0xd8], CARRY_SET, CF_SET_OF_CLEAR)
            .register(Eax, 0x8000_0001, 0xc000_0000),
        Case::new("RCL AL,1; zero input", &[0xd0, 0xd0], CARRY_SET, CF_CLEAR_OF_CLEAR)
            .register(Eax, 0x4433_2200, 0x4433_2201),
        Case::new("RCL AX,1; zero input", &[0x66, 0xd1, 0xd0], CARRY_SET, CF_CLEAR_OF_CLEAR)
            .register(Eax, 0x4433_0000, 0x4433_0001),
        Case::new("RCL EAX,1; zero input", &[0xd1, 0xd0], CARRY_SET, CF_CLEAR_OF_CLEAR)
            .register(Eax, 0x0000_0000, 0x0000_0001),
        Case::new("RCR AL,1; zero input", &[0xd0, 0xd8], CARRY_SET, CF_CLEAR_OF_SET)
            .register(Eax, 0x4433_2200, 0x4433_2280),
        Case::new("RCR AX,1; zero input", &[0x66, 0xd1, 0xd8], CARRY_SET, CF_CLEAR_OF_SET)
            .register(Eax, 0x4433_0000, 0x4433_8000),
        Case::new("RCR EAX,1; zero input", &[0xd1, 0xd8], CARRY_SET, CF_CLEAR_OF_SET)
            .register(Eax, 0x0000_0000, 0x8000_0000),
        Case::new("RCL AL,1; all bits set", &[0xd0, 0xd0], CARRY_CLEAR, CF_SET_OF_CLEAR)
            .register(Eax, 0x4433_22ff, 0x4433_22fe),
        Case::new("RCL AX,1; all bits set", &[0x66, 0xd1, 0xd0], CARRY_CLEAR, CF_SET_OF_CLEAR)
            .register(Eax, 0x4433_ffff, 0x4433_fffe),
        Case::new("RCL EAX,1; all bits set", &[0xd1, 0xd0], CARRY_CLEAR, CF_SET_OF_CLEAR)
            .register(Eax, 0xffff_ffff, 0xffff_fffe),
        Case::new("RCR AL,1; all bits set", &[0xd0, 0xd8], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x4433_22ff, 0x4433_227f),
        Case::new("RCR AX,1; all bits set", &[0x66, 0xd1, 0xd8], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x4433_ffff, 0x4433_7fff),
        Case::new("RCR EAX,1; all bits set", &[0xd1, 0xd8], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0xffff_ffff, 0x7fff_ffff),
    ]
}

#[rustfmt::skip]
pub(super) fn alias_cases() -> Vec<Case> {
    vec![
        Case::new("RCL CL,CL; CF=0", &[0xd2, 0xd1], CARRY_CLEAR, CF_CLEAR_OF_CLEAR)
            .register(Ecx, 0x8877_6603, 0x8877_6618),
        Case::new("RCL CL,CL; CF=1", &[0xd2, 0xd1], CARRY_SET, CF_CLEAR_OF_CLEAR)
            .register(Ecx, 0x8877_6603, 0x8877_661c),
        Case::new("RCR CH,CL; CF=0", &[0xd2, 0xdd], CARRY_CLEAR, CF_CLEAR_OF_SET)
            .register(Ecx, 0x8877_8001, 0x8877_4001),
        Case::new("RCR CH,CL; CF=1", &[0xd2, 0xdd], CARRY_SET, CF_CLEAR_OF_CLEAR)
            .register(Ecx, 0x8877_8001, 0x8877_c001),
        Case::new("RCL CX,CL; CF=0", &[0x66, 0xd3, 0xd1], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Ecx, 0x8877_8001, 0x8877_0002),
        Case::new("RCL CX,CL; CF=1", &[0x66, 0xd3, 0xd1], CARRY_SET, CF_SET_OF_SET)
            .register(Ecx, 0x8877_8001, 0x8877_0003),
        Case::new("RCR ECX,CL; CF=0", &[0xd3, 0xd9], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Ecx, 0x8000_0001, 0x4000_0000),
        Case::new("RCR ECX,CL; CF=1", &[0xd3, 0xd9], CARRY_SET, CF_SET_OF_CLEAR)
            .register(Ecx, 0x8000_0001, 0xc000_0000),
        Case::new("RCL AH,CL; CF=0", &[0xd2, 0xd4], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Eax, 0x4433_8111, 0x4433_0211)
            .initial_register(Ecx, 0x8877_6601),
        Case::new("RCL AH,CL; CF=1", &[0xd2, 0xd4], CARRY_SET, CF_SET_OF_SET)
            .register(Eax, 0x4433_8111, 0x4433_0311)
            .initial_register(Ecx, 0x8877_6601),
        Case::new("RCR BH,1 with 66; CF=0", &[0x66, 0xd0, 0xdf], CARRY_CLEAR, CF_SET_OF_SET)
            .register(Ebx, 0x10ff_81dd, 0x10ff_40dd)
            .initial_register(Ecx, 0x8877_6601),
        Case::new("RCR BH,1 with 66; CF=1", &[0x66, 0xd0, 0xdf], CARRY_SET, CF_SET_OF_CLEAR)
            .register(Ebx, 0x10ff_81dd, 0x10ff_c0dd)
            .initial_register(Ecx, 0x8877_6601),
        Case::new("RCL SP,17; CF=0", &[0x66, 0xc1, 0xd4, 0x11], CARRY_CLEAR, FLAGS_PRESERVED)
            .register(Esp, 0x8765_8010, 0x8765_8010)
            .initial_register(Ecx, 0x8877_6611),
        Case::new("RCL SP,17; CF=1", &[0x66, 0xc1, 0xd4, 0x11], CARRY_SET, FLAGS_PRESERVED)
            .register(Esp, 0x8765_8010, 0x8765_8010)
            .initial_register(Ecx, 0x8877_6611),
    ]
}
