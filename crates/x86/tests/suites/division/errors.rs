use wasm86_x86::Gpr32::{Eax, Ebx, Edx};

use crate::support::cases::{test_cases, InstructionCase as Case};

fn zero_divisors() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, accumulator, high, divisor) in [
        (
            "DIV byte",
            &[0xf6, 0xf3][..],
            0x4433_0037,
            0xccbb_aa99,
            0x10ff_ee00,
        ),
        (
            "IDIV byte",
            &[0xf6, 0xfb][..],
            0x4433_ff9c,
            0xccbb_aa99,
            0x10ff_ee00,
        ),
        (
            "DIV word",
            &[0x66, 0xf7, 0xf3][..],
            0x4433_0037,
            0xccbb_0000,
            0x10ff_0000,
        ),
        (
            "IDIV word",
            &[0x66, 0xf7, 0xfb][..],
            0x4433_ff9c,
            0xccbb_ffff,
            0x10ff_0000,
        ),
        ("DIV dword", &[0xf7, 0xf3][..], 1, 0, 0),
        ("IDIV dword", &[0xf7, 0xfb][..], 0xffff_ff9c, 0xffff_ffff, 0),
    ] {
        cases.push(
            Case::preserving_flags(format!("{name} zero divisor"), code)
                .initial_registers(&[(Eax, accumulator), (Edx, high), (Ebx, divisor)])
                .divide_error(),
        );
        cases.push(
            Case::preserving_flags(format!("{name} zero divided by zero still faults"), code)
                .initial_registers(&[(Eax, 0), (Edx, 0), (Ebx, divisor)])
                .divide_error(),
        );
    }
    cases
}

#[rustfmt::skip]
fn unsigned_overflow() -> Vec<Case> {
    vec![
        Case::preserving_flags("DIV byte first quotient above the unsigned range", &[0xf6, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0100), (Ebx, 0x10ff_ee01)]).divide_error(),
        Case::preserving_flags("DIV byte high half exceeds a nonzero divisor", &[0xf6, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0401), (Ebx, 0x10ff_ee03)]).divide_error(),
        Case::preserving_flags("DIV byte maximum dividend exceeds maximum divisor range", &[0xf6, 0xf3])
            .initial_registers(&[(Eax, 0x4433_ffff), (Ebx, 0x10ff_eeff)]).divide_error(),
        Case::preserving_flags("DIV word first quotient above the unsigned range", &[0x66, 0xf7, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0000), (Edx, 0xccbb_0001), (Ebx, 0x10ff_0001)]).divide_error(),
        Case::preserving_flags("DIV word high half exceeds a nonzero divisor", &[0x66, 0xf7, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0001), (Edx, 0xccbb_0004), (Ebx, 0x10ff_0003)]).divide_error(),
        Case::preserving_flags("DIV word maximum dividend exceeds maximum divisor range", &[0x66, 0xf7, 0xf3])
            .initial_registers(&[(Eax, 0x4433_ffff), (Edx, 0xccbb_ffff), (Ebx, 0x10ff_ffff)]).divide_error(),
        Case::preserving_flags("DIV dword first quotient above the unsigned range", &[0xf7, 0xf3])
            .initial_registers(&[(Eax, 0), (Edx, 1), (Ebx, 1)]).divide_error(),
        Case::preserving_flags("DIV dword high half exceeds a nonzero divisor", &[0xf7, 0xf3])
            .initial_registers(&[(Eax, 1), (Edx, 4), (Ebx, 3)]).divide_error(),
        Case::preserving_flags("DIV dword maximum dividend exceeds maximum divisor range", &[0xf7, 0xf3])
            .initial_registers(&[(Eax, 0xffff_ffff), (Edx, 0xffff_ffff), (Ebx, 0xffff_ffff)]).divide_error(),
        Case::preserving_flags("DIV old AH is zero", &[0xf6, 0xf4])
            .initial_register(Eax, 0x4433_0080).divide_error(),
        Case::preserving_flags("DIV old DX is zero despite nonzero upper EDX", &[0x66, 0xf7, 0xf2])
            .initial_registers(&[(Eax, 0x4433_8000), (Edx, 0xccbb_0000)]).divide_error(),
        Case::preserving_flags("DIV old EDX is zero", &[0xf7, 0xf2])
            .initial_registers(&[(Eax, 0x8000_0000), (Edx, 0)]).divide_error(),
    ]
}

#[rustfmt::skip]
fn signed_overflow() -> Vec<Case> {
    vec![
        Case::preserving_flags("IDIV byte first quotient above the signed maximum", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_0080), (Ebx, 0x10ff_ee01)]).divide_error(),
        Case::preserving_flags("IDIV byte first quotient below the signed minimum", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_ff7f), (Ebx, 0x10ff_ee01)]).divide_error(),
        Case::preserving_flags("IDIV byte negating the minimum quotient overflows", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_ff80), (Ebx, 0x10ff_eeff)]).divide_error(),
        Case::preserving_flags("IDIV byte minimum double-width dividend divided by negative one", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_8000), (Ebx, 0x10ff_eeff)]).divide_error(),
        Case::preserving_flags("IDIV word first quotient above the signed maximum", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_8000), (Edx, 0xccbb_0000), (Ebx, 0x10ff_0001)]).divide_error(),
        Case::preserving_flags("IDIV word first quotient below the signed minimum", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_7fff), (Edx, 0xccbb_ffff), (Ebx, 0x10ff_0001)]).divide_error(),
        Case::preserving_flags("IDIV word negating the minimum quotient overflows", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_8000), (Edx, 0xccbb_ffff), (Ebx, 0x10ff_ffff)]).divide_error(),
        Case::preserving_flags("IDIV word minimum double-width dividend divided by negative one", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_0000), (Edx, 0xccbb_8000), (Ebx, 0x10ff_ffff)]).divide_error(),
        Case::preserving_flags("IDIV dword first quotient above the signed maximum", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x8000_0000), (Edx, 0), (Ebx, 1)]).divide_error(),
        Case::preserving_flags("IDIV dword first quotient below the signed minimum", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x7fff_ffff), (Edx, 0xffff_ffff), (Ebx, 1)]).divide_error(),
        Case::preserving_flags("IDIV dword negating the minimum quotient overflows", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x8000_0000), (Edx, 0xffff_ffff), (Ebx, 0xffff_ffff)]).divide_error(),
        Case::preserving_flags("IDIV dword minimum double-width dividend divided by negative one", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0), (Edx, 0x8000_0000), (Ebx, 0xffff_ffff)]).divide_error(),
        Case::preserving_flags("IDIV old AH negates the minimum quotient", &[0xf6, 0xfc])
            .initial_register(Eax, 0x4433_ff80).divide_error(),
        Case::preserving_flags("IDIV old DX negates the minimum quotient", &[0x66, 0xf7, 0xfa])
            .initial_registers(&[(Eax, 0x4433_8000), (Edx, 0xccbb_ffff)]).divide_error(),
        Case::preserving_flags("IDIV old EDX negates the minimum quotient", &[0xf7, 0xfa])
            .initial_registers(&[(Eax, 0x8000_0000), (Edx, 0xffff_ffff)]).divide_error(),
    ]
}

test_cases!(
    zero_divisors_preserve_every_architectural_field,
    zero_divisors()
);
test_cases!(
    unsigned_quotients_must_fit_before_any_result_is_written,
    unsigned_overflow()
);
test_cases!(
    signed_quotient_failures_preserve_the_fault_boundary,
    signed_overflow()
);
