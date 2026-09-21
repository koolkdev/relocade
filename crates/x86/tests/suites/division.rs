use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edx, Esi};

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set, Undefined},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};

// Successful DIV/IDIV leave all six status flags architecturally undefined.
// Start with a valid logical record so a policy that preserves it is also valid.
fn division(name: impl Into<String>, code: &[u8]) -> Case {
    Case::new(
        name,
        code,
        Flags {
            cf: true,
            pf: false,
            af: true,
            zf: true,
            sf: false,
            of: false,
        },
        Flags::all(Undefined),
    )
}

#[rustfmt::skip]
fn unsigned_results() -> Vec<Case> {
    vec![
        division("DIV byte zero dividend", &[0xf6, 0xf3])
            .register(Eax, 0x4433_0000, 0x4433_0000).initial_register(Ebx, 0x10ff_eeff),
        division("DIV byte uses both dividend bytes", &[0xf6, 0xf3])
            .register(Eax, 0x4433_0101, 0x4433_0255).initial_register(Ebx, 0x10ff_ee03),
        division("DIV byte maximum quotient has a nonzero remainder", &[0xf6, 0xf3])
            .register(Eax, 0x4433_02ff, 0x4433_02ff).initial_register(Ebx, 0x10ff_ee03),
        division("DIV byte quotient zero retains the dividend as remainder", &[0xf6, 0xf3])
            .register(Eax, 0x4433_007f, 0x4433_7f00).initial_register(Ebx, 0x10ff_ee80),
        division("DIV word uses both dividend halves", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_0001, 0x4433_5555).register(Edx, 0xccbb_0001, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0003),
        division("DIV word maximum quotient has a nonzero remainder", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_ffff, 0x4433_ffff).register(Edx, 0xccbb_0002, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0003),
        division("DIV word unsigned maximum divisor and quotient", &[0x66, 0xf7, 0xf3])
            .register(Eax, 0x4433_0001, 0x4433_ffff).register(Edx, 0xccbb_fffe, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        division("DIV dword uses both dividend halves", &[0xf7, 0xf3])
            .register(Eax, 1, 0x5555_5555).register(Edx, 1, 2).initial_register(Ebx, 3),
        division("DIV dword maximum quotient has a nonzero remainder", &[0xf7, 0xf3])
            .register(Eax, 0xffff_ffff, 0xffff_ffff).register(Edx, 2, 2).initial_register(Ebx, 3),
        division("DIV dword unsigned maximum divisor and quotient", &[0xf7, 0xf3])
            .register(Eax, 1, 0xffff_ffff).register(Edx, 0xffff_fffe, 0).initial_register(Ebx, 0xffff_ffff),
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
        division("IDIV byte negative dividend smaller than divisor", &[0xf6, 0xfb])
            .register(Eax, 0x4433_fffe, 0x4433_fe00).initial_register(Ebx, 0x10ff_ee07),
        division("IDIV byte minimum quotient with negative remainder", &[0xf6, 0xfb])
            .register(Eax, 0x4433_fe7e, 0x4433_fe80).initial_register(Ebx, 0x10ff_ee03),
        division("IDIV byte maximum quotient with positive remainder", &[0xf6, 0xfb])
            .register(Eax, 0x4433_017f, 0x4433_027f).initial_register(Ebx, 0x10ff_ee03),
        division("IDIV byte minimum divisor is signed", &[0xf6, 0xfb])
            .register(Eax, 0x4433_ff80, 0x4433_0001).initial_register(Ebx, 0x10ff_ee80),
        division("IDIV word minimum quotient with negative remainder", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_7ffe, 0x4433_8000).register(Edx, 0xccbb_fffe, 0xccbb_fffe)
            .initial_register(Ebx, 0x10ff_0003),
        division("IDIV word maximum quotient with positive remainder", &[0x66, 0xf7, 0xfb])
            .register(Eax, 0x4433_7fff, 0x4433_7fff).register(Edx, 0xccbb_0001, 0xccbb_0002)
            .initial_register(Ebx, 0x10ff_0003),
        division("IDIV dword minimum quotient with negative remainder", &[0xf7, 0xfb])
            .register(Eax, 0x7fff_fffe, 0x8000_0000).register(Edx, 0xffff_fffe, 0xffff_fffe).initial_register(Ebx, 3),
        division("IDIV dword maximum quotient with positive remainder", &[0xf7, 0xfb])
            .register(Eax, 0x7fff_ffff, 0x7fff_ffff).register(Edx, 1, 2).initial_register(Ebx, 3),
        division("IDIV dword negative dividend extends beyond one dword", &[0xf7, 0xfb])
            .register(Eax, 0xffff_ffff, 0xaaaa_aaab).register(Edx, 0xffff_fffe, 0xffff_fffe).initial_register(Ebx, 3),
    ]
}

#[rustfmt::skip]
fn divisor_aliases() -> Vec<Case> {
    vec![
        division("DIV captures old AL before replacing quotient and remainder", &[0xf6, 0xf0])
            .register(Eax, 0x4433_0130, 0x4433_1006),
        division("IDIV captures old AH as a signed divisor", &[0xf6, 0xfc])
            .register(Eax, 0x4433_ff9c, 0x4433_0064),
        Case::preserving_flags("DIV old AH equal to the high dividend byte always overflows", &[0xf6, 0xf4])
            .initial_register(Eax, 0x4433_0101).divide_error(),
        division("DIV captures old AX before replacing both dividend halves", &[0x66, 0xf7, 0xf0])
            .register(Eax, 0x4433_0030, 0x4433_0556).register(Edx, 0xccbb_0001, 0xccbb_0010),
        division("IDIV captures old DX as a negative divisor", &[0x66, 0xf7, 0xfa])
            .register(Eax, 0x4433_ff9c, 0x4433_0064).register(Edx, 0xccbb_ffff, 0xccbb_0000),
        division("IDIV captures old EAX as a negative divisor", &[0xf7, 0xf8])
            .register(Eax, 0xffff_ff9c, 1).register(Edx, 0xffff_ffff, 0),
        Case::preserving_flags("DIV old EDX equal to the high dividend half always overflows", &[0xf7, 0xf2])
            .initial_registers(&[(Eax, 1), (Edx, 1)]).divide_error(),
    ]
}

#[rustfmt::skip]
fn divide_errors() -> Vec<Case> {
    vec![
        Case::preserving_flags("DIV byte zero divisor", &[0xf6, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0037), (Ebx, 0x10ff_ee00)]).divide_error(),
        Case::preserving_flags("IDIV byte zero divided by zero", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_0000), (Ebx, 0x10ff_ee00)]).divide_error(),
        Case::preserving_flags("DIV word zero divided by zero", &[0x66, 0xf7, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0000), (Edx, 0xccbb_0000), (Ebx, 0x10ff_0000)]).divide_error(),
        Case::preserving_flags("IDIV word zero divisor", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_ff9c), (Edx, 0xccbb_ffff), (Ebx, 0x10ff_0000)]).divide_error(),
        Case::preserving_flags("DIV dword zero divided by zero", &[0xf7, 0xf3])
            .initial_registers(&[(Eax, 0), (Edx, 0), (Ebx, 0)]).divide_error(),
        Case::preserving_flags("IDIV dword zero divisor", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0xffff_ff9c), (Edx, 0xffff_ffff), (Ebx, 0)]).divide_error(),
        Case::preserving_flags("DIV byte first quotient above the unsigned range", &[0xf6, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0100), (Ebx, 0x10ff_ee01)]).divide_error(),
        Case::preserving_flags("DIV word first quotient above the unsigned range", &[0x66, 0xf7, 0xf3])
            .initial_registers(&[(Eax, 0x4433_0000), (Edx, 0xccbb_0001), (Ebx, 0x10ff_0001)]).divide_error(),
        Case::preserving_flags("DIV dword first quotient above the unsigned range", &[0xf7, 0xf3])
            .initial_registers(&[(Eax, 0), (Edx, 1), (Ebx, 1)]).divide_error(),
        Case::preserving_flags("DIV dword high half exceeds a nonzero divisor", &[0xf7, 0xf3])
            .initial_registers(&[(Eax, 1), (Edx, 4), (Ebx, 3)]).divide_error(),
        Case::preserving_flags("IDIV byte first quotient above the signed maximum", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_0080), (Ebx, 0x10ff_ee01)]).divide_error(),
        Case::preserving_flags("IDIV byte first quotient below the signed minimum", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_ff7f), (Ebx, 0x10ff_ee01)]).divide_error(),
        Case::preserving_flags("IDIV byte minimum double-width dividend divided by negative one", &[0xf6, 0xfb])
            .initial_registers(&[(Eax, 0x4433_8000), (Ebx, 0x10ff_eeff)]).divide_error(),
        Case::preserving_flags("IDIV word first quotient above the signed maximum", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_8000), (Edx, 0xccbb_0000), (Ebx, 0x10ff_0001)]).divide_error(),
        Case::preserving_flags("IDIV word first quotient below the signed minimum", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_7fff), (Edx, 0xccbb_ffff), (Ebx, 0x10ff_0001)]).divide_error(),
        Case::preserving_flags("IDIV word minimum double-width dividend divided by negative one", &[0x66, 0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x4433_0000), (Edx, 0xccbb_8000), (Ebx, 0x10ff_ffff)]).divide_error(),
        Case::preserving_flags("IDIV dword first quotient above the signed maximum", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x8000_0000), (Edx, 0), (Ebx, 1)]).divide_error(),
        Case::preserving_flags("IDIV dword first quotient below the signed minimum", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0x7fff_ffff), (Edx, 0xffff_ffff), (Ebx, 1)]).divide_error(),
        Case::preserving_flags("IDIV dword minimum double-width dividend divided by negative one", &[0xf7, 0xfb])
            .initial_registers(&[(Eax, 0), (Edx, 0x8000_0000), (Ebx, 0xffff_ffff)]).divide_error(),
        Case::preserving_flags("IDIV old EDX negates the minimum quotient", &[0xf7, 0xfa])
            .initial_registers(&[(Eax, 0x8000_0000), (Edx, 0xffff_ffff)]).divide_error(),
    ]
}

#[rustfmt::skip]
fn memory_sources() -> Vec<Case> {
    vec![
        division("DIV byte reads only the final mapped byte", &[0xf6, 0x33])
            .register(Eax, 0x4433_0101, 0x4433_0255).initial_register(Ebx, 0x4fff)
            .memory(0x4fff, &[3], ReadOnly),
        division("IDIV word reads its complete divisor across scattered pages", &[0x66, 0xf7, 0x3b])
            .register(Eax, 0x4433_ff9c, 0x4433_000e).register(Edx, 0xccbb_ffff, 0xccbb_fffe)
            .initial_register(Ebx, 0x4fff).map_page(4, 0x8000, ReadOnly).map_page(5, 0xa000, ReadOnly)
            .memory(0x4fff, &[0xf9, 0xff], ReadOnly),
        division("IDIV byte captures the EAX address before writing a negative quotient", &[0xf6, 0x38])
            .register(Eax, 0x8000_4001, 0x8000_0180)
            .memory(0x8000_4000, &[0x5a, 0x80, 0xa5], ReadOnly),
        division("DIV word uses every address bit and preserves both upper halves", &[0x66, 0xf7, 0x30])
            .register(Eax, 0x8000_4000, 0x8000_6aaa).register(Edx, 0xccbb_0001, 0xccbb_0002)
            .memory(0x8000_3fff, &[0x5a, 3, 0, 0xa5], ReadOnly),
        division("DIV dword captures the EDX source address before writing the remainder", &[0xf7, 0x32])
            .register(Eax, 0x1234, 0x4020_0000).register(Edx, 0x4020, 0x1234)
            .memory(0x401f, &[0x5a, 0, 0, 1, 0, 0xa5], ReadOnly),
    ]
}

#[rustfmt::skip]
fn memory_faults() -> Vec<Case> {
    vec![
        Case::preserving_flags("DIV readable zero divisor raises divide error", &[0xf6, 0x33])
            .initial_registers(&[(Eax, 0x4433_0101), (Ebx, 0x4fff)])
            .memory(0x4fff, &[0], ReadOnly).divide_error(),
        Case::preserving_flags("IDIV complete split source causes quotient overflow", &[0xf7, 0x3b])
            .initial_registers(&[(Eax, 0), (Edx, 0x8000_0000), (Ebx, 0x4fff)])
            .memory(0x4fff, &[0xff; 4], ReadOnly).divide_error(),
        Case::preserving_flags("DIV impossible quotient still requires a readable byte divisor", &[0xf6, 0x33])
            .initial_registers(&[(Eax, 0x4433_ffff), (Ebx, 0x5000)]).fault(0x5000, 0),
        Case::preserving_flags("DIV visible zero low byte cannot replace a complete divisor read", &[0x66, 0xf7, 0x33])
            .initial_registers(&[(Eax, 0x4433_0000), (Edx, 0xccbb_0001), (Ebx, 0x4fff)])
            .memory(0x4fff, &[0], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("IDIV signed minimum still requires the complete divisor", &[0xf7, 0x3b])
            .initial_registers(&[(Eax, 0), (Edx, 0x8000_0000), (Ebx, 0x4fff)])
            .memory(0x4fff, &[0xff], ReadOnly).fault(0x5000, 0),
        Case::preserving_flags("DIV zero dividend still reads its divisor", &[0xf7, 0x33])
            .initial_registers(&[(Eax, 0), (Edx, 0), (Ebx, 0x5000)]).fault(0x5000, 0),
    ]
}

// Keeping the incoming flag record is a software policy, not an x86 guarantee.
#[rustfmt::skip]
fn undefined_flags_policy() -> Vec<Case> {
    vec![
        Case::preserving_flags("DIV byte software policy preserves opaque undefined flags", &[0xf6, 0xf3])
            .register(Eax, 0x4433_0101, 0x4433_0255).initial_register(Ebx, 0x10ff_ee03),
        Case::preserving_flags("IDIV dword software policy preserves opaque undefined flags", &[0xf7, 0xfb])
            .register(Eax, 0xffff_ff9c, 0xffff_fff2).register(Edx, 0xffff_ffff, 0xffff_fffe).initial_register(Ebx, 7),
    ]
}

#[rustfmt::skip]
fn division_sequences() -> Vec<Sequence> {
    vec![
        Sequence::new("consecutive divisions consume both prior results before a later overflow", Flags::all(true))
            .initial_registers(&[(Eax, 1), (Edx, 1), (Ebx, 3)])
            .step(Step::new(&[0xf7, 0xf3], Flags::all(Undefined))
                .register(Eax, 0x5555_5555).register(Edx, 2))
            .step(Step::new(&[0xf7, 0xf0], Flags::all(Undefined)).register(Eax, 7).register(Edx, 2))
            .step(Step::preserving_flags(&[0xf7, 0xf2]).divide_error())
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Sequence::from_opaque_flags("divide error publishes the preceding ADD and memory store")
            .initial_registers(&[(Eax, 0x1234_5678), (Edx, 0x9876_5432), (Ebx, 0x7fff_ffff), (Ecx, 0), (Esi, 0x4000)])
            .memory(0x4000, &[0xa5, 0xa5, 0xa5, 0xa5], ReadWrite)
            .step(Step::new(&[0x83, 0xc3, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ebx, 0x8000_0000))
            .step(Step::preserving_flags(&[0x89, 0x1e]).expect_memory(0x4000, &[0, 0, 0, 0x80]))
            .step(Step::preserving_flags(&[0xf7, 0xf9]).divide_error())
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Sequence::new("a source fault publishes the preceding signed quotient and remainder", Flags::all(false))
            .initial_registers(&[(Eax, 0xffff_ff9c), (Edx, 0xffff_ffff), (Ebx, 7), (Esi, 0x5000)])
            .step(Step::new(&[0xf7, 0xfb], Flags::all(Undefined))
                .register(Eax, 0xffff_fff2).register(Edx, 0xffff_fffe))
            .step(Step::preserving_flags(&[0xf7, 0x36]).fault(0x5000, 0))
            .trailing_code(&[0xb8, 0, 0, 0, 0], 1),
        Sequence::preserving_flags("constant zero dividend and divisor raise divide error after three MOVs")
            .step(Step::preserving_flags(&[0xb8, 0, 0, 0, 0]).register(Eax, 0))
            .step(Step::preserving_flags(&[0xba, 0, 0, 0, 0]).register(Edx, 0))
            .step(Step::preserving_flags(&[0xb9, 0, 0, 0, 0]).register(Ecx, 0))
            .step(Step::preserving_flags(&[0xf7, 0xf1]).divide_error())
            .trailing_code(&[0xb8, 1, 0, 0, 0], 1),
        Sequence::preserving_flags("constant signed double-width minimum over negative one preserves the MOV results")
            .step(Step::preserving_flags(&[0xb8, 0, 0, 0, 0]).register(Eax, 0))
            .step(Step::preserving_flags(&[0xba, 0, 0, 0, 0x80]).register(Edx, 0x8000_0000))
            .step(Step::preserving_flags(&[0xb9, 0xff, 0xff, 0xff, 0xff]).register(Ecx, 0xffff_ffff))
            .step(Step::preserving_flags(&[0xf7, 0xf9]).divide_error())
            .trailing_code(&[0xba, 0, 0, 0, 0], 1),
        Sequence::preserving_flags("constant byte zero divisor preserves the preceding AX and BL writes")
            .initial_registers(&[(Eax, 0x4433_ffff), (Ebx, 0x10ff_eeff)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0, 0]).register(Eax, 0x4433_0000))
            .step(Step::preserving_flags(&[0xb3, 0]).register(Ebx, 0x10ff_ee00))
            .step(Step::preserving_flags(&[0xf6, 0xfb]).divide_error())
            .trailing_code(&[0x66, 0xb8, 1, 0], 1),
        Sequence::preserving_flags("constant byte minimum dividend over negative one preserves the MOV results")
            .initial_registers(&[(Eax, 0x4433_ffff), (Ebx, 0x10ff_ee00)])
            .step(Step::preserving_flags(&[0x66, 0xb8, 0, 0x80]).register(Eax, 0x4433_8000))
            .step(Step::preserving_flags(&[0xb3, 0xff]).register(Ebx, 0x10ff_eeff))
            .step(Step::preserving_flags(&[0xf6, 0xfb]).divide_error())
            .trailing_code(&[0x66, 0xb8, 0, 0], 1),
    ]
}

#[rustfmt::skip]
fn pending_flags_policy() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("division software policy preserves a pending ADD recipe for later conditions")
            .initial_registers(&[(Eax, 0x4433_0101), (Ebx, 3), (Ecx, 0x7fff_ffff), (Edx, 0)])
            .step(Step::new(&[0x83, 0xc1, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Ecx, 0x8000_0000))
            .step(Step::preserving_flags(&[0xf6, 0xf3]).register(Eax, 0x4433_0255))
            .step(Step::preserving_flags(&[0x0f, 0x90, 0xc2]).register(Edx, 1))
            .step(Step::preserving_flags(&[0x0f, 0x9a, 0xc6]).register(Edx, 0x101)),
    ]
}

#[test]
fn complete_divisor_encodings_have_no_immediate_or_successor_dependency() {
    for code in [
        &[0xf6, 0xf0][..],
        &[0xf6, 0xfc][..],
        &[0x66, 0xf7, 0x34, 0x8b][..],
        &[0x66, 0xf7, 0xbc, 0x8b, 0x20, 0x40, 0, 0][..],
        &[0xf7, 0x75, 0x80][..],
        &[0xf7, 0x3d, 0x20, 0x40, 0, 0][..],
        &[0x66, 0x66, 0xf6, 0xf3][..],
    ] {
        check_length(code);
    }
}

#[test]
fn source_encoding_fetch_precedes_divide_error() {
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1ffb;
    image.cpu.registers.eax = 0;
    image.cpu.registers.edx = 0x8000_0000;
    // IDIV [disp32] is missing its last displacement byte.
    image.data(0x3ffb, &[0xf7, 0x3d, 0x20, 0x40, 0]);
    image.check_unchanged_exit(
        Engine::Wasmtime,
        TestModule::interpreter(),
        "division requires its full source encoding before checking the dividend",
        Exit::PageFault {
            address: 0x2000,
            error: 0x10,
        },
    );
}

test_cases!(unsigned_dividends_and_quotient_limits, unsigned_results());
test_cases!(signed_quotients_and_remainders, signed_results());
test_cases!(old_divisor_aliases, divisor_aliases());
test_cases!(divide_errors_preserve_entry_state, divide_errors());
test_cases!(readonly_sources_and_address_aliases, memory_sources());
test_cases!(complete_reads_precede_divide_errors, memory_faults());
test_cases!(
    undefined_flags_follow_software_policy,
    undefined_flags_policy()
);
test_sequences!(
    quotients_remainders_and_fault_publication,
    division_sequences()
);
test_sequences!(
    undefined_flags_preserve_pending_arithmetic,
    pending_flags_policy()
);
