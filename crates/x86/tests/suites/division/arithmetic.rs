use wasm86_x86::Gpr32::{Eax, Ebx, Edx};

use crate::support::cases::{test_cases, InstructionCase as Case};

use super::successful_division as division;

#[rustfmt::skip]
fn unsigned_results() -> Vec<Case> {
    vec![
        division("DIV byte zero dividend", &[0xf6, 0xf3])
            .register(Eax, 0x4433_0000, 0x4433_0000).initial_register(Ebx, 0x10ff_eeff),
        division("DIV byte unit dividend", &[0xf6, 0xf3])
            .register(Eax, 0x4433_0001, 0x4433_0001).initial_register(Ebx, 0x10ff_ee01),
        division("DIV byte uses both dividend bytes", &[0xf6, 0xf3])
            .register(Eax, 0x4433_0101, 0x4433_0255).initial_register(Ebx, 0x10ff_ee03),
        division("DIV byte maximum quotient has a nonzero remainder", &[0xf6, 0xf3])
            .register(Eax, 0x4433_02ff, 0x4433_02ff).initial_register(Ebx, 0x10ff_ee03),
        division("DIV byte unsigned maximum divisor and quotient", &[0xf6, 0xf3])
            .register(Eax, 0x4433_fe01, 0x4433_00ff).initial_register(Ebx, 0x10ff_eeff),
        division("DIV byte quotient zero retains the dividend as remainder", &[0xf6, 0xf3])
            .register(Eax, 0x4433_007f, 0x4433_7f00).initial_register(Ebx, 0x10ff_ee80),
        division("DIV word zero dividend", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_0000, 0x4433_0000).register(Edx, 0xccbb_0000, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        division("DIV word unit dividend", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_0001, 0x4433_0001).register(Edx, 0xccbb_0000, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_0001),
        division("DIV word uses both dividend halves", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_0001, 0x4433_5555).register(Edx, 0xccbb_0001, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0003),
        division("DIV word maximum quotient has a nonzero remainder", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_ffff, 0x4433_ffff).register(Edx, 0xccbb_0002, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0003),
        division("DIV word unsigned maximum divisor and quotient", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_0001, 0x4433_ffff).register(Edx, 0xccbb_fffe, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        division("DIV word quotient zero retains the dividend as remainder", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_7fff, 0x4433_0000).register(Edx, 0xccbb_0000, 0xccbb_7fff)
            .initial_register(Ebx, 0x10ff_8000),
        division("DIV dword zero dividend", &[0xf7, 0xf3])
            .register(Eax, 0, 0).register(Edx, 0, 0).initial_register(Ebx, 0xffff_ffff),
        division("DIV dword unit dividend", &[0xf7, 0xf3])
            .register(Eax, 1, 1).register(Edx, 0, 0).initial_register(Ebx, 1),
        division("DIV dword uses both dividend halves", &[0xf7, 0xf3])
            .register(Eax, 1, 0x5555_5555).register(Edx, 1, 2).initial_register(Ebx, 3),
        division("DIV dword maximum quotient has a nonzero remainder", &[0xf7, 0xf3])
            .register(Eax, 0xffff_ffff, 0xffff_ffff).register(Edx, 2, 2).initial_register(Ebx, 3),
        division("DIV dword unsigned maximum divisor and quotient", &[0xf7, 0xf3])
            .register(Eax, 1, 0xffff_ffff).register(Edx, 0xffff_fffe, 0).initial_register(Ebx, 0xffff_ffff),
        division("DIV dword quotient zero retains the dividend as remainder", &[0xf7, 0xf3])
            .register(Eax, 0x7fff_ffff, 0).register(Edx, 0, 0x7fff_ffff).initial_register(Ebx, 0x8000_0000),
    ]
}

#[rustfmt::skip]
fn signed_results() -> Vec<Case> {
    vec![
        division("IDIV byte positive dividend and divisor", &[0xf6, 0xfb])
            .register(Eax, 0x4433_0064, 0x4433_020e).initial_register(Ebx, 0x10ff_ee07),
        division("IDIV byte negative dividend truncates toward zero", &[0xf6, 0xfb])
            .register(Eax, 0x4433_ff9c, 0x4433_fef2).initial_register(Ebx, 0x10ff_ee07),
        division("IDIV byte negative divisor keeps a positive remainder", &[0xf6, 0xfb])
            .register(Eax, 0x4433_0064, 0x4433_02f2).initial_register(Ebx, 0x10ff_eef9),
        division("IDIV byte two negative operands keep a negative remainder", &[0xf6, 0xfb])
            .register(Eax, 0x4433_ff9c, 0x4433_fe0e).initial_register(Ebx, 0x10ff_eef9),
        division("IDIV byte zero dividend and negative divisor", &[0xf6, 0xfb])
            .register(Eax, 0x4433_0000, 0x4433_0000).initial_register(Ebx, 0x10ff_ee80),
        division("IDIV byte negative dividend smaller than divisor", &[0xf6, 0xfb])
            .register(Eax, 0x4433_fffe, 0x4433_fe00).initial_register(Ebx, 0x10ff_ee07),
        division("IDIV byte minimum quotient fits exactly", &[0xf6, 0xfb])
            .register(Eax, 0x4433_ff80, 0x4433_0080).initial_register(Ebx, 0x10ff_ee01),
        division("IDIV byte maximum quotient fits exactly", &[0xf6, 0xfb])
            .register(Eax, 0x4433_007f, 0x4433_007f).initial_register(Ebx, 0x10ff_ee01),
        division("IDIV byte minimum quotient with negative remainder", &[0xf6, 0xfb])
            .register(Eax, 0x4433_fe7e, 0x4433_fe80).initial_register(Ebx, 0x10ff_ee03),
        division("IDIV byte maximum quotient with positive remainder", &[0xf6, 0xfb])
            .register(Eax, 0x4433_017f, 0x4433_027f).initial_register(Ebx, 0x10ff_ee03),
        division("IDIV byte minimum divisor is signed", &[0xf6, 0xfb])
            .register(Eax, 0x4433_ff80, 0x4433_0001).initial_register(Ebx, 0x10ff_ee80),
        division("IDIV word positive dividend and divisor", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_0064, 0x4433_000e).register(Edx, 0xccbb_0000, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0007),
        division("IDIV word negative dividend truncates toward zero", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_ff9c, 0x4433_fff2).register(Edx, 0xccbb_ffff, 0xccbb_fffe)
            .initial_register(Ebx, 0x10ff_0007),
        division("IDIV word negative divisor keeps a positive remainder", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_0064, 0x4433_fff2).register(Edx, 0xccbb_0000, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_fff9),
        division("IDIV word two negative operands keep a negative remainder", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_ff9c, 0x4433_000e).register(Edx, 0xccbb_ffff, 0xccbb_fffe)
            .initial_register(Ebx, 0x10ff_fff9),
        division("IDIV word zero dividend and negative divisor", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_0000, 0x4433_0000).register(Edx, 0xccbb_0000, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_8000),
        division("IDIV word negative dividend smaller than divisor", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_fffe, 0x4433_0000).register(Edx, 0xccbb_ffff, 0xccbb_fffe)
            .initial_register(Ebx, 0x10ff_0007),
        division("IDIV word minimum quotient fits exactly", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_8000, 0x4433_8000).register(Edx, 0xccbb_ffff, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_0001),
        division("IDIV word maximum quotient fits exactly", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_7fff, 0x4433_7fff).register(Edx, 0xccbb_0000, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_0001),
        division("IDIV word minimum quotient with negative remainder", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_7ffe, 0x4433_8000).register(Edx, 0xccbb_fffe, 0xccbb_fffe)
            .initial_register(Ebx, 0x10ff_0003),
        division("IDIV word maximum quotient with positive remainder", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_7fff, 0x4433_7fff).register(Edx, 0xccbb_0001, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0003),
        division("IDIV word minimum divisor is signed", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_8000, 0x4433_0001).register(Edx, 0xccbb_ffff, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_8000),
        division("IDIV dword positive dividend and divisor", &[0xf7, 0xfb])
            .register(Eax, 100, 14).register(Edx, 0, 2).initial_register(Ebx, 7),
        division("IDIV dword negative dividend truncates toward zero", &[0xf7, 0xfb])
            .register(Eax, 0xffff_ff9c, 0xffff_fff2).register(Edx, 0xffff_ffff, 0xffff_fffe).initial_register(Ebx, 7),
        division("IDIV dword negative divisor keeps a positive remainder", &[0xf7, 0xfb])
            .register(Eax, 100, 0xffff_fff2).register(Edx, 0, 2).initial_register(Ebx, 0xffff_fff9),
        division("IDIV dword two negative operands keep a negative remainder", &[0xf7, 0xfb])
            .register(Eax, 0xffff_ff9c, 14).register(Edx, 0xffff_ffff, 0xffff_fffe).initial_register(Ebx, 0xffff_fff9),
        division("IDIV dword zero dividend and negative divisor", &[0xf7, 0xfb])
            .register(Eax, 0, 0).register(Edx, 0, 0).initial_register(Ebx, 0x8000_0000),
        division("IDIV dword negative dividend smaller than divisor", &[0xf7, 0xfb])
            .register(Eax, 0xffff_fffe, 0).register(Edx, 0xffff_ffff, 0xffff_fffe).initial_register(Ebx, 7),
        division("IDIV dword minimum quotient fits exactly", &[0xf7, 0xfb])
            .register(Eax, 0x8000_0000, 0x8000_0000).register(Edx, 0xffff_ffff, 0).initial_register(Ebx, 1),
        division("IDIV dword maximum quotient fits exactly", &[0xf7, 0xfb])
            .register(Eax, 0x7fff_ffff, 0x7fff_ffff).register(Edx, 0, 0).initial_register(Ebx, 1),
        division("IDIV dword minimum quotient with negative remainder", &[0xf7, 0xfb])
            .register(Eax, 0x7fff_fffe, 0x8000_0000).register(Edx, 0xffff_fffe, 0xffff_fffe).initial_register(Ebx, 3),
        division("IDIV dword maximum quotient with positive remainder", &[0xf7, 0xfb])
            .register(Eax, 0x7fff_ffff, 0x7fff_ffff).register(Edx, 1, 2).initial_register(Ebx, 3),
        division("IDIV dword minimum divisor is signed", &[0xf7, 0xfb])
            .register(Eax, 0x8000_0000, 1).register(Edx, 0xffff_ffff, 0).initial_register(Ebx, 0x8000_0000),
        division("IDIV dword negative dividend extends beyond one dword", &[0xf7, 0xfb])
            .register(Eax, 0xffff_ffff, 0xaaaa_aaab).register(Edx, 0xffff_fffe, 0xffff_fffe).initial_register(Ebx, 3),
    ]
}

// Keeping the incoming flag record is a software policy, not an x86 guarantee.
#[rustfmt::skip]
fn undefined_flags_policy() -> Vec<Case> {
    vec![
        Case::preserving_flags("DIV byte software policy preserves opaque undefined flags", &[0xf6, 0xf3])
            .register(Eax, 0x4433_0101, 0x4433_0255).initial_register(Ebx, 0x10ff_ee03),
        Case::preserving_flags("IDIV byte software policy preserves opaque undefined flags", &[0xf6, 0xfb])
            .register(Eax, 0x4433_ff9c, 0x4433_fef2).initial_register(Ebx, 0x10ff_ee07),
        Case::preserving_flags("DIV word software policy preserves opaque undefined flags", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_0001, 0x4433_5555).register(Edx, 0xccbb_0001, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0003),
        Case::preserving_flags("IDIV word software policy preserves opaque undefined flags", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_ff9c, 0x4433_fff2).register(Edx, 0xccbb_ffff, 0xccbb_fffe)
            .initial_register(Ebx, 0x10ff_0007),
        Case::preserving_flags("DIV dword software policy preserves opaque undefined flags", &[0xf7, 0xf3])
            .register(Eax, 1, 0x5555_5555).register(Edx, 1, 2).initial_register(Ebx, 3),
        Case::preserving_flags("IDIV dword software policy preserves opaque undefined flags", &[0xf7, 0xfb])
            .register(Eax, 0xffff_ff9c, 0xffff_fff2).register(Edx, 0xffff_ffff, 0xffff_fffe).initial_register(Ebx, 7),
    ]
}

test_cases!(
    unsigned_full_dividends_and_quotient_limits,
    unsigned_results()
);
test_cases!(signed_quotients_and_remainder_signs, signed_results());
test_cases!(
    undefined_flags_follow_software_preservation_policy,
    undefined_flags_policy()
);
