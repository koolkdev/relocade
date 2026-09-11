use wasm86_x86::Gpr32::{Eax, Ebx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};

// Encoding and alias coverage uses independently reviewed literal results.
#[rustfmt::skip]
fn cases() -> Vec<Case> {
    vec![
        Case::new("ADC AL, register source", &[0x10, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("ADC AL, reverse register form", &[0x12, 0xc3], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("ADC AL, accumulator immediate -1", &[0x14, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("ADC AL, full group immediate -1", &[0x80, 0xd0, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("ADC AL,ff ignores 66", &[0x66, 0x14, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("ADC AX, register source", &[0x66, 0x11, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("ADC AX, reverse register form", &[0x66, 0x13, 0xc3], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("ADC AX, accumulator immediate -1", &[0x66, 0x15, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("ADC AX, full group immediate -1", &[0x66, 0x81, 0xd0, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("ADC AX, sign-extended immediate 7f", &[0x66, 0x83, 0xd0, 0x7f], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0081).initial_register(Ebx, 0x0000_007f),
        Case::new("ADC AX, sign-extended immediate 80", &[0x66, 0x83, 0xd0, 0x80], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_ff82).initial_register(Ebx, 0x0000_ff80),
        Case::new("ADC AX, sign-extended immediate ff", &[0x66, 0x83, 0xd0, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("ADC EAX, register source", &[0x11, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("ADC EAX, reverse register form", &[0x13, 0xc3], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("ADC EAX, accumulator immediate -1", &[0x15, 0xff, 0xff, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("ADC EAX, full group immediate -1", &[0x81, 0xd0, 0xff, 0xff, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("ADC EAX, sign-extended immediate 7f", &[0x83, 0xd0, 0x7f], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0081).initial_register(Ebx, 0x0000_007f),
        Case::new("ADC EAX, sign-extended immediate 80", &[0x83, 0xd0, 0x80], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x0000_0001, 0xffff_ff82).initial_register(Ebx, 0xffff_ff80),
        Case::new("ADC EAX, sign-extended immediate ff", &[0x83, 0xd0, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("ADC AL,AH reads old byte aliases", &[0x10, 0xe0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_7f80, 0x4433_7f00),
        Case::new("ADC AH,AL reads old byte aliases", &[0x10, 0xc4], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear })
            .register(Eax, 0x4433_7f80, 0x4433_0080),
        Case::new("ADC AH,AH reads old byte aliases", &[0x10, 0xe4], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_7f80, 0x4433_ff80),
        Case::new("ADC AX,AX reads its old self operand", &[0x66, 0x11, 0xc0], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_8000, 0x8000_0001),
        Case::new("ADC EAX,EAX reads its old self operand", &[0x11, 0xc0], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0x8000_8000, 0x0001_0001),
        Case::new("SBB AL, register source", &[0x18, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("SBB AL, reverse register form", &[0x1a, 0xc3], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("SBB AL, accumulator immediate -1", &[0x1c, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("SBB AL, full group immediate -1", &[0x80, 0xd8, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("SBB AL,ff ignores 66", &[0x66, 0x1c, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2201, 0x4433_2201).initial_register(Ebx, 0x0000_00ff),
        Case::new("SBB AX, register source", &[0x66, 0x19, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("SBB AX, reverse register form", &[0x66, 0x1b, 0xc3], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("SBB AX, accumulator immediate -1", &[0x66, 0x1d, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("SBB AX, full group immediate -1", &[0x66, 0x81, 0xd8, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("SBB AX, sign-extended immediate 7f", &[0x66, 0x83, 0xd8, 0x7f], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_ff81).initial_register(Ebx, 0x0000_007f),
        Case::new("SBB AX, sign-extended immediate 80", &[0x66, 0x83, 0xd8, 0x80], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0080).initial_register(Ebx, 0x0000_ff80),
        Case::new("SBB AX, sign-extended immediate ff", &[0x66, 0x83, 0xd8, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x0000_ffff),
        Case::new("SBB EAX, register source", &[0x19, 0xd8], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("SBB EAX, reverse register form", &[0x1b, 0xc3], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("SBB EAX, accumulator immediate -1", &[0x1d, 0xff, 0xff, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("SBB EAX, full group immediate -1", &[0x81, 0xd8, 0xff, 0xff, 0xff, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("SBB EAX, sign-extended immediate 7f", &[0x83, 0xd8, 0x7f], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x0000_0001, 0xffff_ff81).initial_register(Ebx, 0x0000_007f),
        Case::new("SBB EAX, sign-extended immediate 80", &[0x83, 0xd8, 0x80], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0080).initial_register(Ebx, 0xffff_ff80),
        Case::new("SBB EAX, sign-extended immediate ff", &[0x83, 0xd8, 0xff], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Set, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x0000_0001, 0x0000_0001).initial_register(Ebx, 0xffff_ffff),
        Case::new("SBB AL,AH reads old byte aliases", &[0x18, 0xe0], Flags::all(true),
            Flags { cf: Clear, pf: Set, af: Set, zf: Set, sf: Clear, of: Set })
            .register(Eax, 0x4433_7f80, 0x4433_7f00),
        Case::new("SBB AH,AL reads old byte aliases", &[0x18, 0xc4], Flags::all(true),
            Flags { cf: Set, pf: Clear, af: Clear, zf: Clear, sf: Set, of: Set })
            .register(Eax, 0x4433_7f80, 0x4433_fe80),
        Case::new("SBB AH,AH reads old byte aliases", &[0x18, 0xe4], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x4433_7f80, 0x4433_ff80),
        Case::new("SBB AX,AX reads its old self operand", &[0x66, 0x19, 0xc0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_8000, 0x8000_ffff),
        Case::new("SBB EAX,EAX reads its old self operand", &[0x19, 0xc0], Flags::all(true),
            Flags { cf: Set, pf: Set, af: Set, zf: Clear, sf: Set, of: Clear })
            .register(Eax, 0x8000_8000, 0xffff_ffff),
    ]
}

test_cases!(register_immediate_and_alias_forms, cases());
