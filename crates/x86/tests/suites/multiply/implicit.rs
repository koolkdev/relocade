use wasm86_x86::Gpr32::{self, Eax, Ebx, Ecx, Edx};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{Clear, Set},
    Flags, InstructionCase as Case,
};

use super::product_flags;

#[rustfmt::skip]
fn edge_cases() -> Vec<Case> {
    vec![
        Case::replacing_flags("MUL byte zero", &[0xf6, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_2200, 0x4433_0000).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("MUL byte maximum fits", &[0xf6, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_2201, 0x4433_00ff).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("MUL byte maximum squared", &[0xf6, 0xe3], product_flags(Set))
            .register(Eax, 0x4433_22ff, 0x4433_fe01).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("MUL byte first product above the width", &[0xf6, 0xe3], product_flags(Set))
            .register(Eax, 0x4433_2280, 0x4433_0100).initial_register(Ebx, 0x10ff_ee02),
        Case::replacing_flags("MUL byte unsigned sign bit still fits", &[0xf6, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_227f, 0x4433_00fe).initial_register(Ebx, 0x10ff_ee02),
        Case::replacing_flags("IMUL byte zero", &[0xf6, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_2200, 0x4433_0000).initial_register(Ebx, 0x10ff_ee80),
        Case::replacing_flags("IMUL byte negative one fits", &[0xf6, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_22ff, 0x4433_ffff).initial_register(Ebx, 0x10ff_ee01),
        Case::replacing_flags("IMUL byte minimum fits", &[0xf6, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_2280, 0x4433_ff80).initial_register(Ebx, 0x10ff_ee01),
        Case::replacing_flags("IMUL byte negating the minimum overflows", &[0xf6, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_2280, 0x4433_0080).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("IMUL byte positive result needs another sign bit", &[0xf6, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_227f, 0x4433_00fe).initial_register(Ebx, 0x10ff_ee02),
        Case::replacing_flags("IMUL byte negative one squared", &[0xf6, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_22ff, 0x4433_0001).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("IMUL byte minimum squared", &[0xf6, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_2280, 0x4433_4000).initial_register(Ebx, 0x10ff_ee80),
        Case::replacing_flags("MUL word zero", &[0x66, 0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_0000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("MUL word maximum fits", &[0x66, 0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_0001, 0x4433_ffff).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("MUL word maximum squared", &[0x66, 0xf7, 0xe3], product_flags(Set))
            .register(Eax, 0x4433_ffff, 0x4433_0001).register(Edx, 0xccbb_aa99, 0xccbb_fffe)
            .initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("MUL word first product above the width", &[0x66, 0xf7, 0xe3], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_0001)
            .initial_register(Ebx, 0x10ff_0002),
        Case::replacing_flags("MUL word unsigned sign bit still fits", &[0x66, 0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0x4433_7fff, 0x4433_fffe).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_0002),
        Case::replacing_flags("IMUL word zero", &[0x66, 0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_0000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_8000),
        Case::replacing_flags("IMUL word negative one fits", &[0x66, 0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_ffff, 0x4433_ffff).register(Edx, 0xccbb_aa99, 0xccbb_ffff)
            .initial_register(Ebx, 0x10ff_0001),
        Case::replacing_flags("IMUL word minimum fits", &[0x66, 0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_8000, 0x4433_8000).register(Edx, 0xccbb_aa99, 0xccbb_ffff)
            .initial_register(Ebx, 0x10ff_0001),
        Case::replacing_flags("IMUL word negating the minimum overflows", &[0x66, 0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_8000).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("IMUL word positive result needs another sign bit", &[0x66, 0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_7fff, 0x4433_fffe).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_0002),
        Case::replacing_flags("IMUL word negative one squared", &[0x66, 0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0x4433_ffff, 0x4433_0001).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .initial_register(Ebx, 0x10ff_ffff),
        Case::replacing_flags("IMUL word minimum squared", &[0x66, 0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_4000)
            .initial_register(Ebx, 0x10ff_8000),
        Case::replacing_flags("MUL dword zero", &[0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0, 0).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 0xffff_ffff),
        Case::replacing_flags("MUL dword maximum fits", &[0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 1, 0xffff_ffff).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 0xffff_ffff),
        Case::replacing_flags("MUL dword maximum squared", &[0xf7, 0xe3], product_flags(Set))
            .register(Eax, 0xffff_ffff, 1).register(Edx, 0xccbb_aa99, 0xffff_fffe)
            .initial_register(Ebx, 0xffff_ffff),
        Case::replacing_flags("MUL dword first product above the width", &[0xf7, 0xe3], product_flags(Set))
            .register(Eax, 0x8000_0000, 0).register(Edx, 0xccbb_aa99, 1).initial_register(Ebx, 2),
        Case::replacing_flags("MUL dword unsigned sign bit still fits", &[0xf7, 0xe3], product_flags(Clear))
            .register(Eax, 0x7fff_ffff, 0xffff_fffe).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 2),
        Case::replacing_flags("IMUL dword zero", &[0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0, 0).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 0x8000_0000),
        Case::replacing_flags("IMUL dword negative one fits", &[0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0xffff_ffff, 0xffff_ffff).register(Edx, 0xccbb_aa99, 0xffff_ffff)
            .initial_register(Ebx, 1),
        Case::replacing_flags("IMUL dword minimum fits", &[0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0x8000_0000, 0x8000_0000).register(Edx, 0xccbb_aa99, 0xffff_ffff)
            .initial_register(Ebx, 1),
        Case::replacing_flags("IMUL dword negating the minimum overflows", &[0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x8000_0000, 0x8000_0000).register(Edx, 0xccbb_aa99, 0)
            .initial_register(Ebx, 0xffff_ffff),
        Case::replacing_flags("IMUL dword positive result needs another sign bit", &[0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x7fff_ffff, 0xffff_fffe).register(Edx, 0xccbb_aa99, 0).initial_register(Ebx, 2),
        Case::replacing_flags("IMUL dword negative one squared", &[0xf7, 0xeb], product_flags(Clear))
            .register(Eax, 0xffff_ffff, 1).register(Edx, 0xccbb_aa99, 0)
            .initial_register(Ebx, 0xffff_ffff),
        Case::replacing_flags("IMUL dword minimum squared", &[0xf7, 0xeb], product_flags(Set))
            .register(Eax, 0x8000_0000, 0).register(Edx, 0xccbb_aa99, 0x4000_0000)
            .initial_register(Ebx, 0x8000_0000),
        Case::replacing_flags("IMUL byte captures negative AH before replacing AX", &[0xf6, 0xec], product_flags(Clear))
            .register(Eax, 0x4433_ff03, 0x4433_fffd),
        Case::replacing_flags("IMUL byte old AH negates the minimum", &[0xf6, 0xec], product_flags(Set))
            .register(Eax, 0x4433_ff80, 0x4433_0080),
        Case::replacing_flags("IMUL byte captures negative AL before squaring it", &[0xf6, 0xe8], product_flags(Clear))
            .register(Eax, 0x4433_22ff, 0x4433_0001),
        Case::replacing_flags("MUL word captures maximum DX before replacing both halves", &[0x66, 0xf7, 0xe2], product_flags(Set))
            .register(Eax, 0x4433_ffff, 0x4433_0001).register(Edx, 0xccbb_ffff, 0xccbb_fffe),
        Case::replacing_flags("IMUL word old DX produces a negative high half", &[0x66, 0xf7, 0xea], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_0002, 0xccbb_ffff),
        Case::replacing_flags("IMUL word captures minimum AX before squaring it", &[0x66, 0xf7, 0xe8], product_flags(Set))
            .register(Eax, 0x4433_8000, 0x4433_0000).register(Edx, 0xccbb_aa99, 0xccbb_4000),
        Case::replacing_flags("MUL dword captures maximum EDX before replacing both halves", &[0xf7, 0xe2], product_flags(Set))
            .register(Eax, 0xffff_ffff, 1).register(Edx, 0xffff_ffff, 0xffff_fffe),
        Case::replacing_flags("IMUL dword old EDX produces a negative high half", &[0xf7, 0xea], product_flags(Set))
            .register(Eax, 0x8000_0000, 0).register(Edx, 2, 0xffff_ffff),
        Case::replacing_flags("IMUL dword captures minimum EAX before squaring it", &[0xf7, 0xe8], product_flags(Set))
            .register(Eax, 0x8000_0000, 0).register(Edx, 0xccbb_aa99, 0x4000_0000),
    ]
}

fn byte_selectors() -> Vec<Case> {
    let mut cases = Vec::new();
    for (mnemonic, extension) in [("MUL", 4), ("IMUL", 5)] {
        for (code, source, input, accumulator, product) in [
            (0, Eax, 0x4433_0302, 0x4433_0302, 0x4433_0004),
            (1, Ecx, 0x8877_6603, 0x4433_2202, 0x4433_0006),
            (2, Edx, 0xccbb_aa03, 0x4433_2202, 0x4433_0006),
            (3, Ebx, 0x10ff_ee03, 0x4433_2202, 0x4433_0006),
            (4, Eax, 0x4433_0302, 0x4433_0302, 0x4433_0006),
            (5, Ecx, 0x8877_0355, 0x4433_2202, 0x4433_0006),
            (6, Edx, 0xccbb_0399, 0x4433_2202, 0x4433_0006),
            (7, Ebx, 0x10ff_03dd, 0x4433_2202, 0x4433_0006),
        ] {
            let mut case = Case::replacing_flags(
                format!("{mnemonic} byte selector {code} captures its old source"),
                &[0xf6, 0xc0 | (extension << 3) | code],
                product_flags(Clear),
            )
            .register(Eax, accumulator, product);
            if source != Eax {
                case = case.initial_register(source, input);
            }
            cases.push(case);
        }
    }
    cases
}

fn wide_selectors() -> Vec<Case> {
    struct Products {
        width: &'static str,
        prefix: &'static [u8],
        sources: [u32; 8],
        accumulator: u32,
        high_input: u32,
        high_output: u32,
        square: u32,
        product: u32,
    }

    let mut cases = Vec::new();
    for (mnemonic, extension) in [("MUL", 4), ("IMUL", 5)] {
        for products in [
            Products {
                width: "word",
                prefix: &[0x66],
                sources: [
                    0x4433_0002,
                    0x8877_0003,
                    0xccbb_0003,
                    0x10ff_0003,
                    0x8765_0003,
                    0x6789_0003,
                    0x7654_0003,
                    0x89ab_0003,
                ],
                accumulator: 0x4433_0002,
                high_input: 0xccbb_aa99,
                high_output: 0xccbb_0000,
                square: 0x4433_0004,
                product: 0x4433_0006,
            },
            Products {
                width: "dword",
                prefix: &[],
                sources: [2, 3, 3, 3, 3, 3, 3, 3],
                accumulator: 2,
                high_input: 0xccbb_aa99,
                high_output: 0,
                square: 4,
                product: 6,
            },
        ] {
            for (code, (source, input)) in Gpr32::ALL.into_iter().zip(products.sources).enumerate()
            {
                let mut encoding = products.prefix.to_vec();
                encoding.extend_from_slice(&[0xf7, 0xc0 | (extension << 3) | code as u8]);
                let mut case = Case::replacing_flags(
                    format!(
                        "{mnemonic} {} source {source:?} reads the old accumulator and source",
                        products.width
                    ),
                    &encoding,
                    product_flags(Clear),
                )
                .register(
                    Eax,
                    products.accumulator,
                    if source == Eax {
                        products.square
                    } else {
                        products.product
                    },
                )
                .register(
                    Edx,
                    if source == Edx {
                        input
                    } else {
                        products.high_input
                    },
                    products.high_output,
                );
                if source != Eax && source != Edx {
                    case = case.initial_register(source, input);
                }
                cases.push(case);
            }
        }
    }
    cases
}

// These exact undefined-flag values are a software policy, not x86 guarantees.
#[rustfmt::skip]
fn undefined_flags_policy() -> Vec<Case> {
    vec![
        Case::replacing_flags("MUL zero follows software policy with ZF clear", &[0xf6, 0xe3],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_2200, 0x4433_0000).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("IMUL one follows software policy with PF set", &[0xf6, 0xeb],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_22ff, 0x4433_0001).initial_register(Ebx, 0x10ff_eeff),
        Case::replacing_flags("IMUL negative word follows software policy with SF clear", &[0x66, 0xf7, 0xeb],
            Flags { cf: Clear, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Clear })
            .register(Eax, 0x4433_ffff, 0x4433_ffff).register(Edx, 0xccbb_aa99, 0xccbb_ffff)
            .initial_register(Ebx, 0x10ff_0001),
        Case::replacing_flags("MUL dword overflow retains the same undefined-flag software policy", &[0xf7, 0xe3],
            Flags { cf: Set, pf: Set, af: Clear, zf: Clear, sf: Clear, of: Set })
            .register(Eax, 0xffff_ffff, 1).register(Edx, 0xccbb_aa99, 0xffff_fffe)
            .initial_register(Ebx, 0xffff_ffff),
    ]
}

test_cases!(full_products_and_signed_fit, edge_cases());
test_cases!(all_low_and_high_byte_sources, byte_selectors());
test_cases!(all_word_and_dword_sources, wide_selectors());
test_cases!(
    undefined_flags_follow_software_policy,
    undefined_flags_policy()
);
