//! Decimal corrections use literal results, including non-BCD input boundaries.

#[path = "decimal_adjust/decoding.rs"]
mod decoding;
#[path = "decimal_adjust/radix.rs"]
mod radix;
#[path = "decimal_adjust/sequences.rs"]
mod sequences;

use crate::support::cases::{
    test_cases,
    FlagExpectation::{self, Clear, Preserved, Set, Undefined},
    Flags, InstructionCase as Case,
};
use wasm86_x86::Gpr32::Eax;

fn flag(value: bool) -> FlagExpectation {
    if value {
        Set
    } else {
        Clear
    }
}

fn unpacked_flags(adjusted: bool) -> Flags<FlagExpectation> {
    Flags {
        cf: flag(adjusted),
        af: flag(adjusted),
        ..Flags::all(Undefined)
    }
}

fn digit_flags(result: u8) -> Flags<FlagExpectation> {
    Flags {
        pf: flag(result.count_ones().is_multiple_of(2)),
        zf: flag(result == 0),
        sf: flag(result & 0x80 != 0),
        ..Flags::all(Undefined)
    }
}

fn packed_flags(result: u8, carry: bool, auxiliary: bool) -> Flags<FlagExpectation> {
    Flags {
        cf: flag(carry),
        af: flag(auxiliary),
        ..digit_flags(result)
    }
}

struct UnpackedExample {
    ax: u16,
    auxiliary: bool,
    aaa: u16,
    aas: u16,
    adjusted: bool,
}

#[rustfmt::skip]
fn unpacked_corrections() -> Vec<Case> {
    let examples = [
        UnpackedExample { ax: 0x1209, auxiliary: false, aaa: 0x1209, aas: 0x1209, adjusted: false },
        UnpackedExample { ax: 0x12f9, auxiliary: false, aaa: 0x1209, aas: 0x1209, adjusted: false },
        UnpackedExample { ax: 0x120a, auxiliary: false, aaa: 0x1300, aas: 0x1104, adjusted: true },
        UnpackedExample { ax: 0x120f, auxiliary: false, aaa: 0x1305, aas: 0x1109, adjusted: true },
        UnpackedExample { ax: 0x1200, auxiliary: true,  aaa: 0x1306, aas: 0x100a, adjusted: true },
        UnpackedExample { ax: 0x1205, auxiliary: true,  aaa: 0x130b, aas: 0x100f, adjusted: true },
        UnpackedExample { ax: 0x1206, auxiliary: true,  aaa: 0x130c, aas: 0x1100, adjusted: true },
        UnpackedExample { ax: 0x12fa, auxiliary: false, aaa: 0x1400, aas: 0x1104, adjusted: true },
        UnpackedExample { ax: 0x12ff, auxiliary: false, aaa: 0x1405, aas: 0x1109, adjusted: true },
        UnpackedExample { ax: 0xfffa, auxiliary: false, aaa: 0x0100, aas: 0xfe04, adjusted: true },
        UnpackedExample { ax: 0x0000, auxiliary: true,  aaa: 0x0106, aas: 0xfe0a, adjusted: true },
        UnpackedExample { ax: 0xff09, auxiliary: true,  aaa: 0x000f, aas: 0xfe03, adjusted: true },
        UnpackedExample { ax: 0x12f9, auxiliary: true,  aaa: 0x130f, aas: 0x1103, adjusted: true },
    ];
    let mut cases = Vec::new();
    for example in examples {
        for (opcode, result) in [(0x37, example.aaa), (0x3f, example.aas)] {
            for carry in [false, true] {
                cases.push(Case::new(
                    format!("{opcode:02x} AX={:04x}, AF={}, CF={carry}", example.ax, example.auxiliary),
                    &[opcode],
                    Flags { af: example.auxiliary, ..Flags::all(carry) },
                    unpacked_flags(example.adjusted),
                ).register(Eax, 0x4433_0000 | u32::from(example.ax), 0x4433_0000 | u32::from(result)));
            }
        }
    }
    cases
}

struct PackedExample {
    al: u8,
    carry: bool,
    auxiliary: bool,
    daa: u8,
    das: u8,
    daa_carry: bool,
    das_carry: bool,
    adjusted_low: bool,
}

#[rustfmt::skip]
fn packed_corrections() -> Vec<Case> {
    let examples = [
        PackedExample { al: 0x00, carry: false, auxiliary: false, daa: 0x00, das: 0x00, daa_carry: false, das_carry: false, adjusted_low: false },
        PackedExample { al: 0x09, carry: false, auxiliary: false, daa: 0x09, das: 0x09, daa_carry: false, das_carry: false, adjusted_low: false },
        PackedExample { al: 0x0a, carry: false, auxiliary: false, daa: 0x10, das: 0x04, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x0f, carry: false, auxiliary: false, daa: 0x15, das: 0x09, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x10, carry: false, auxiliary: false, daa: 0x10, das: 0x10, daa_carry: false, das_carry: false, adjusted_low: false },
        PackedExample { al: 0x00, carry: false, auxiliary: true,  daa: 0x06, das: 0xfa, daa_carry: false, das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x05, carry: false, auxiliary: true,  daa: 0x0b, das: 0xff, daa_carry: false, das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x06, carry: false, auxiliary: true,  daa: 0x0c, das: 0x00, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x79, carry: false, auxiliary: false, daa: 0x79, das: 0x79, daa_carry: false, das_carry: false, adjusted_low: false },
        PackedExample { al: 0x7a, carry: false, auxiliary: false, daa: 0x80, das: 0x74, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x99, carry: false, auxiliary: false, daa: 0x99, das: 0x99, daa_carry: false, das_carry: false, adjusted_low: false },
        PackedExample { al: 0x99, carry: false, auxiliary: true,  daa: 0x9f, das: 0x93, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x9a, carry: false, auxiliary: false, daa: 0x00, das: 0x34, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x9f, carry: false, auxiliary: false, daa: 0x05, das: 0x39, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0xa0, carry: false, auxiliary: false, daa: 0x00, das: 0x40, daa_carry: true,  das_carry: true,  adjusted_low: false },
        PackedExample { al: 0xfa, carry: false, auxiliary: false, daa: 0x60, das: 0x94, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0xff, carry: false, auxiliary: false, daa: 0x65, das: 0x99, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x00, carry: true,  auxiliary: false, daa: 0x60, das: 0xa0, daa_carry: true,  das_carry: true,  adjusted_low: false },
        PackedExample { al: 0x09, carry: true,  auxiliary: false, daa: 0x69, das: 0xa9, daa_carry: true,  das_carry: true,  adjusted_low: false },
        PackedExample { al: 0x15, carry: true,  auxiliary: true,  daa: 0x7b, das: 0xaf, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x99, carry: true,  auxiliary: true,  daa: 0xff, das: 0x33, daa_carry: true,  das_carry: true,  adjusted_low: true },
    ];
    let mut cases = Vec::new();
    for example in examples {
        for (opcode, result, carry) in [(0x27, example.daa, example.daa_carry), (0x2f, example.das, example.das_carry)] {
            for prior_other_flags in [false, true] {
                cases.push(Case::new(
                    format!("{opcode:02x} AL={:02x}, AF={}, CF={}, other flags={prior_other_flags}", example.al, example.auxiliary, example.carry),
                    &[opcode],
                    Flags { cf: example.carry, af: example.auxiliary, ..Flags::all(prior_other_flags) },
                    packed_flags(result, carry, example.adjusted_low),
                ).register(Eax, 0x4433_2200 | u32::from(example.al), 0x4433_2200 | u32::from(result)));
            }
        }
    }
    cases
}

fn undefined_flag_policy() -> Vec<Case> {
    let mut cases = Vec::new();
    for initial in [false, true] {
        for (code, input, output, mut expected) in [
            (&[0x37][..], 0x000a, 0x0100, unpacked_flags(true)),
            (&[0x3f][..], 0x000a, 0xff04, unpacked_flags(true)),
            (&[0x27][..], 0x009a, 0x0000, packed_flags(0, true, true)),
            (&[0x2f][..], 0x009a, 0x0034, packed_flags(0x34, true, true)),
            (&[0xd4, 10][..], 0x0051, 0x0801, digit_flags(1)),
            (&[0xd5, 10][..], 0x0801, 0x0051, digit_flags(0x51)),
        ] {
            for expectation in [
                &mut expected.cf,
                &mut expected.pf,
                &mut expected.af,
                &mut expected.zf,
                &mut expected.sf,
                &mut expected.of,
            ] {
                if matches!(expectation, Undefined) {
                    *expectation = Preserved;
                }
            }
            cases.push(
                Case::new(
                    format!("{code:02x?} preserves undefined flags initially {initial}"),
                    code,
                    Flags::all(initial),
                    expected,
                )
                .register(Eax, 0x4433_0000 | input, 0x4433_0000 | output),
            );
        }
    }
    cases
}

test_cases!(
    unpacked_corrections_propagate_through_ax,
    unpacked_corrections()
);
test_cases!(packed_corrections_and_decimal_borrows, packed_corrections());
test_cases!(
    undefined_status_flags_are_preserved,
    undefined_flag_policy()
);
