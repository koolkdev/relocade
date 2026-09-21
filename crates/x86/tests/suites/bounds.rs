//! BOUND signed ranges and the ordered reads of its two memory fields.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, BlockError, Gpr32::*, Segment, SegmentAttributes, StoredSegment,
};

#[path = "bounds/memory.rs"]
mod memory;

fn pair(word: bool, lower: i32, upper: i32) -> Vec<u8> {
    let width = if word { 2 } else { 4 };
    [&lower.to_le_bytes()[..width], &upper.to_le_bytes()[..width]].concat()
}

fn code16() -> StoredSegment {
    StoredSegment {
        attributes: SegmentAttributes::from_bits(0x07),
        ..StoredSegment::flat_code32(0x1b)
    }
}

fn segment(base: u32, limit: u32, attributes: u16) -> StoredSegment {
    StoredSegment {
        base,
        limit,
        selector: 0x23,
        attributes: SegmentAttributes::from_bits(attributes),
    }
}

fn absolute_code(word: bool, default16: bool, register: u8, address: u32) -> Vec<u8> {
    let mut code = Vec::new();
    if word != default16 {
        code.push(0x66);
    }
    code.extend([0x62, (register << 3) | if default16 { 6 } else { 5 }]);
    code.extend_from_slice(&address.to_le_bytes()[..if default16 { 2 } else { 4 }]);
    code
}

fn signed_ranges() -> Vec<Case> {
    let mut cases = Vec::new();
    for word in [false, true] {
        let (min, max) = if word {
            (-32768, 32767)
        } else {
            (i32::MIN, i32::MAX)
        };
        // The success column is authored independently of the implementation.
        for (index, lower, upper, success) in [
            (-6, -5, 7, false),
            (-5, -5, 7, true),
            (0, -5, 7, true),
            (7, -5, 7, true),
            (8, -5, 7, false),
            (11, -5, 7, false),
            (-1, -1, -1, true),
            (0, -1, -1, false),
            (-2, -1, -1, false),
            (min, min, max, true),
            (max, min, max, true),
            (min, min + 1, max, false),
            (max, min, max - 1, false),
            (min, max, min, false),
            (0, 7, -5, false),
            (7, 7, -5, false),
        ] {
            let value = if word {
                0xa5a5_0000 | (index as u32 & 0xffff)
            } else {
                index as u32
            };
            let mut case = Case::preserving_flags(
                format!("BOUND word={word}: {lower} <= {index} <= {upper}"),
                &absolute_code(word, false, 0, 0x4000),
            )
            .initial_register(Eax, value)
            .memory(0x4000, &pair(word, lower, upper), ReadOnly);
            if !success {
                case = case.bound_range_exceeded();
            }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn index_forms() -> Vec<Case> {
    vec![
        Case::preserving_flags("EBP supplies the full signed dword index", &absolute_code(false, false, 5, 0x4000))
            .initial_register(Ebp, u32::MAX).memory(0x4000, &pair(false, -2, 0), ReadOnly),
        Case::preserving_flags("SP supplies only its signed low word", &absolute_code(true, false, 4, 0x4000))
            .initial_register(Esp, 0x1234_ffff).memory(0x4000, &pair(true, -2, 0), ReadOnly),
        Case::preserving_flags("CS16 defaults to a word index and word fields", &absolute_code(true, true, 0, 0x4000))
            .segmented_only().segment(Segment::Cs, code16()).initial_register(Eax, 0x1234_ffff)
            .memory(0x4000, &pair(true, -2, 0), ReadOnly),
        Case::preserving_flags("CS16 operand override compares the full EDI", &absolute_code(false, true, 7, 0x4000))
            .segmented_only().segment(Segment::Cs, code16()).initial_register(Edi, 0x1234_ffff)
            .memory(0x4000, &pair(false, -2, 0), ReadOnly).bound_range_exceeded(),
    ]
}
test_cases!(signed_inclusive_ranges, signed_ranges());
test_cases!(index_widths_and_default_sizes, index_forms());

#[test]
fn forms_consume_a_memory_address_without_an_immediate() {
    for code in [
        &[0x62, 0x03][..],
        &[0x66, 0x62, 0x44, 0x8b, 0x80],
        &[0x62, 0x3d, 0, 0x40, 0, 0],
        &[0x67, 0x62, 0x06, 0, 0x40],
    ] {
        check_length(code);
    }
}

#[test]
fn register_operands_are_rejected_without_a_successor_fetch() {
    for code in [&[0x62, 0xc0][..], &[0x66, 0x62, 0xff]] {
        let start = 0x2000 - code.len() as u32;
        assert_eq!(
            compile_block_from_bytes(start, code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: start,
                opcode: 0x62
            })
        );
        let mut image = Image::new(&[]);
        image.cpu.eip = start;
        image.data(0x3000 + (start & 0xfff), code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "BOUND requires a memory pair",
            Exit::Other(0x0008_0062_0000_0000 | u64::from(start)),
        );
    }
}

#[rustfmt::skip]
fn continuation_cases() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("a word bounds fault publishes the preceding store and arithmetic")
            .initial_registers(&[(Eax, 0x1234_7fff), (Ebx, 0x1111_1111)])
            .memory(0x4000, &pair(true, -5, 7), ReadWrite)
            .step(Step::preserving_flags(&[0xbb, 0, 0x40, 0, 0]).register(Ebx, 0x4000))
            .step(Step::preserving_flags(&[0x66, 0xc7, 0x03, 0xfa, 0xff]).expect_memory(0x4000, &[0xfa, 0xff]))
            .step(Step::new(&[0x66, 0x83, 0xc0, 1],
                Flags { cf: Clear, pf: Set, af: Set, zf: Clear, sf: Set, of: Set }).register(Eax, 0x1234_8000))
            .step(Step::preserving_flags(&[0x66, 0x62, 0x03]).bound_range_exceeded())
            .trailing_code(&[0x89, 0xc1], 1),
        Sequence::preserving_flags("a later BOUND reads the changed upper field")
            .initial_registers(&[(Eax, 7), (Ebx, 0x4000)]).memory(0x4000, &pair(false, -5, 7), ReadWrite)
            .step(Step::preserving_flags(&[0x62, 0x03]))
            .step(Step::preserving_flags(&[0xc7, 0x43, 4, 6, 0, 0, 0]).expect_memory(0x4004, &[6, 0, 0, 0]))
            .step(Step::preserving_flags(&[0x62, 0x03]).bound_range_exceeded())
            .trailing_code(&[0x89, 0xc1], 1),
    ]
}
test_sequences!(continuation_and_fault_publication, continuation_cases());
