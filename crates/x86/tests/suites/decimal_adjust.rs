//! Distinct adjustment results, fixed operands, and instruction-specific fault ordering.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set, Undefined},
        Flags, InstructionCase as Case,
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    Gpr32::{Eax, Ebx},
    Segment, SegmentAttributes, StoredSegment,
};

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
        UnpackedExample { ax: 0x1205, auxiliary: true,  aaa: 0x130b, aas: 0x100f, adjusted: true },
        UnpackedExample { ax: 0x1206, auxiliary: true,  aaa: 0x130c, aas: 0x1100, adjusted: true },
        UnpackedExample { ax: 0xfffa, auxiliary: false, aaa: 0x0100, aas: 0xfe04, adjusted: true },
        UnpackedExample { ax: 0x0000, auxiliary: true,  aaa: 0x0106, aas: 0xfe0a, adjusted: true },
        UnpackedExample { ax: 0xff09, auxiliary: true,  aaa: 0x000f, aas: 0xfe03, adjusted: true },
        UnpackedExample { ax: 0x12f9, auxiliary: true,  aaa: 0x130f, aas: 0x1103, adjusted: true },
    ];
    let mut cases = Vec::new();
    for example in examples {
        for (opcode, result) in [(0x37, example.aaa), (0x3f, example.aas)] {
            // Opposite incoming CF checks both setting and clearing without a cross product.
            cases.push(Case::new(
                format!("{opcode:02x} AX={:04x}, AF={}", example.ax, example.auxiliary),
                &[opcode],
                Flags { af: example.auxiliary, ..Flags::all(!example.adjusted) },
                unpacked_flags(example.adjusted),
            ).register(Eax, 0x4433_0000 | u32::from(example.ax), 0x4433_0000 | u32::from(result)));
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
        PackedExample { al: 0x00, carry: false, auxiliary: true,  daa: 0x06, das: 0xfa, daa_carry: false, das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x05, carry: false, auxiliary: true,  daa: 0x0b, das: 0xff, daa_carry: false, das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x06, carry: false, auxiliary: true,  daa: 0x0c, das: 0x00, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x7a, carry: false, auxiliary: false, daa: 0x80, das: 0x74, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x99, carry: false, auxiliary: false, daa: 0x99, das: 0x99, daa_carry: false, das_carry: false, adjusted_low: false },
        PackedExample { al: 0x99, carry: false, auxiliary: true,  daa: 0x9f, das: 0x93, daa_carry: false, das_carry: false, adjusted_low: true },
        PackedExample { al: 0x9a, carry: false, auxiliary: false, daa: 0x00, das: 0x34, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0xa0, carry: false, auxiliary: false, daa: 0x00, das: 0x40, daa_carry: true,  das_carry: true,  adjusted_low: false },
        PackedExample { al: 0xfa, carry: false, auxiliary: false, daa: 0x60, das: 0x94, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0xff, carry: false, auxiliary: false, daa: 0x65, das: 0x99, daa_carry: true,  das_carry: true,  adjusted_low: true },
        PackedExample { al: 0x00, carry: true,  auxiliary: false, daa: 0x60, das: 0xa0, daa_carry: true,  das_carry: true,  adjusted_low: false },
        PackedExample { al: 0x15, carry: true,  auxiliary: true,  daa: 0x7b, das: 0xaf, daa_carry: true,  das_carry: true,  adjusted_low: true },
    ];
    let mut cases = Vec::new();
    for example in examples {
        for (opcode, result, carry) in [(0x27, example.daa, example.daa_carry), (0x2f, example.das, example.das_carry)] {
            cases.push(Case::new(
                format!("{opcode:02x} AL={:02x}, AF={}, CF={}", example.al, example.auxiliary, example.carry),
                &[opcode],
                Flags { cf: example.carry, af: example.auxiliary, ..Flags::all(false) },
                packed_flags(result, carry, example.adjusted_low),
            ).register(Eax, 0x4433_2200 | u32::from(example.al), 0x4433_2200 | u32::from(result)));
        }
    }
    cases
}

struct RadixExample {
    code: [u8; 2],
    ax: u16,
    result: u16,
}

#[rustfmt::skip]
fn conversions() -> Vec<Case> {
    let examples = [
        RadixExample { code: [0xd4, 10], ax: 0xab00, result: 0x0000 },
        RadixExample { code: [0xd4, 10], ax: 0xab09, result: 0x0009 },
        RadixExample { code: [0xd4, 10], ax: 0xab0a, result: 0x0100 },
        RadixExample { code: [0xd4, 10], ax: 0xabff, result: 0x1905 },
        RadixExample { code: [0xd4, 1], ax: 0xabff, result: 0xff00 },
        RadixExample { code: [0xd4, 128], ax: 0xabff, result: 0x017f },
        RadixExample { code: [0xd4, 255], ax: 0xabfe, result: 0x00fe },
        RadixExample { code: [0xd4, 255], ax: 0xabff, result: 0x0100 },
        RadixExample { code: [0xd5, 10], ax: 0x0909, result: 0x0063 },
        RadixExample { code: [0xd5, 10], ax: 0xffff, result: 0x00f5 },
        RadixExample { code: [0xd5, 10], ax: 0x1906, result: 0x0000 },
        RadixExample { code: [0xd5, 0], ax: 0xab80, result: 0x0080 },
        RadixExample { code: [0xd5, 1], ax: 0x01ff, result: 0x0000 },
        RadixExample { code: [0xd5, 128], ax: 0x0201, result: 0x0001 },
        RadixExample { code: [0xd5, 255], ax: 0x0101, result: 0x0000 },
    ];
    examples.into_iter().map(|example| Case::new(
        format!("{:02x?} converts AX={:04x}", example.code, example.ax),
        &example.code, Flags::all(true), digit_flags(example.result as u8),
    ).register(Eax, 0x4433_0000 | u32::from(example.ax), 0x4433_0000 | u32::from(example.result))).collect()
}

fn fixed_operands_and_undefined_flags() -> Vec<Case> {
    let mut cases = Vec::new();
    for (code, input, output, mut expected) in [
        (&[0x37][..], 0x000a, 0x0100, unpacked_flags(true)),
        (&[0x3f][..], 0x000a, 0xff04, unpacked_flags(true)),
        (&[0x27][..], 0x009a, 0x0000, packed_flags(0, true, true)),
        (&[0x2f][..], 0x009a, 0x0034, packed_flags(0x34, true, true)),
        (&[0xd4, 10][..], 0x0051, 0x0801, digit_flags(1)),
        (&[0xd5, 10][..], 0x0801, 0x0051, digit_flags(0x51)),
    ] {
        // Architectural cases leave these flags undefined; these examples protect
        // our preservation policy while checking fixed widths under an override.
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
        let bytes = [&[0x66][..], code].concat();
        for code16 in [false, true] {
            let mut case = Case::new(
                format!("{bytes:02x?} fixes AL/AX and preserves undefined flags, CS.D16={code16}"),
                &bytes,
                Flags::all(code16),
                expected,
            )
            .register(Eax, 0x4433_0000 | input, 0x4433_0000 | output);
            if code16 {
                case = case.segmented_only().segment(
                    Segment::Cs,
                    StoredSegment {
                        attributes: SegmentAttributes::from_bits(0x07),
                        ..StoredSegment::flat_code32(0x1b)
                    },
                );
            }
            cases.push(case);
        }
    }
    cases
}

#[test]
fn encodings_consume_only_their_opcode_and_optional_byte_base() {
    for code in [
        &[0x37][..],
        &[0x3f][..],
        &[0x27][..],
        &[0x2f][..],
        &[0xd4, 0][..],
        &[0xd5, 255][..],
        &[0x66, 0xd4, 10][..],
    ] {
        check_length(code);
    }
}

#[test]
fn interpreter_aam_fetches_its_base_before_testing_for_divide_error() {
    let mut image = Image::new(&[]);
    image.cpu.eip = 0x1fff;
    image.cpu.registers.eax = 0x4433_ab00;
    image.data(0x3fff, &[0xd4]);
    image.check_unchanged_exit(
        Engine::Wasmtime,
        TestModule::interpreter(),
        "an absent AAM base faults before any adjustment or divide error",
        Exit::PageFault {
            address: 0x2000,
            error: 16,
        },
    );
}

fn dependent_adjustments() -> Vec<Sequence> {
    vec![
        Sequence::new(
            "AAA consumes ADD flags, feeds ADC, and survives a later AAM fault",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x4433_1209), (Ebx, 0)])
        .step(
            Step::new(
                &[0x04, 9],
                Flags {
                    af: Set,
                    pf: Set,
                    ..Flags::all(Clear)
                },
            )
            .register(Eax, 0x4433_1212),
        )
        .step(Step::new(&[0x37], unpacked_flags(true)).register(Eax, 0x4433_1308))
        .step(Step::new(&[0x80, 0xd3, 0], Flags::all(Clear)).register(Ebx, 1))
        .step(Step::preserving_flags(&[0xd4, 0]).divide_error())
        .trailing_code(&[0xd5, 10], 1),
        // SUB leaves AL below both correction thresholds, so DAS needs pending AF and CF.
        Sequence::new(
            "DAS consumes pending SUB flags and supplies LAHF",
            Flags::all(false),
        )
        .initial_register(Eax, 0x4433_2200)
        .step(
            Step::new(
                &[0x2c, 0xfa],
                Flags {
                    cf: Set,
                    af: Set,
                    pf: Set,
                    ..Flags::all(Clear)
                },
            )
            .register(Eax, 0x4433_2206),
        )
        .step(Step::new(&[0x2f], packed_flags(0xa0, true, true)).register(Eax, 0x4433_22a0))
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_97a0)),
        Sequence::new(
            "AAD reads a preceding AH write and AAM reads its result",
            Flags::all(true),
        )
        .initial_register(Eax, 0x4433_0009)
        .step(Step::preserving_flags(&[0xb4, 9]).register(Eax, 0x4433_0909))
        .step(Step::new(&[0xd5, 10], digit_flags(0x63)).register(Eax, 0x4433_0063))
        .step(Step::new(&[0xd4, 10], digit_flags(9)).register(Eax, 0x4433_0909)),
    ]
}

test_cases!(unpacked_correction_boundaries, unpacked_corrections());
test_cases!(packed_correction_boundaries, packed_corrections());
test_cases!(encoded_bases_and_byte_wrapping, conversions());
test_cases!(
    fixed_operands_and_undefined_flag_policy,
    fixed_operands_and_undefined_flags()
);
test_cases!(
    aam_zero_base,
    [Case::preserving_flags(
        "AAM base zero faults without fetching a successor",
        &[0xd4, 0]
    )
    .at(0x1ffe)
    .initial_register(Eax, 0x4433_ab00)
    .divide_error(),]
);
test_sequences!(
    dependent_flags_registers_and_faults,
    dependent_adjustments()
);
