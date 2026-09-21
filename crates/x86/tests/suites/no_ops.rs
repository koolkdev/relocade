//! Multi-byte NOP consumes its address encoding without accessing an operand.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{Gpr32::*, Segment, SegmentAttributes, StoredSegment};

// Intel SDM Volume 2, NOP: recommended alignment encodings of three to nine bytes.
const ALIGNMENT_NOPS: &[&[u8]] = &[
    &[0x0f, 0x1f, 0x00],
    &[0x0f, 0x1f, 0x40, 0x00],
    &[0x0f, 0x1f, 0x44, 0x00, 0x00],
    &[0x66, 0x0f, 0x1f, 0x44, 0x00, 0x00],
    &[0x0f, 0x1f, 0x80, 0x00, 0x00, 0x00, 0x00],
    &[0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
    &[0x66, 0x0f, 0x1f, 0x84, 0x00, 0x00, 0x00, 0x00, 0x00],
];

fn complete_forms() -> Vec<Case> {
    let mut cases: Vec<_> = ALIGNMENT_NOPS
        .iter()
        .map(|code| {
            Case::preserving_flags(format!("{}-byte alignment NOP", code.len()), code)
                .initial_register(Eax, 0x4000)
        })
        .collect();
    cases.extend([
        Case::preserving_flags("register form preserves EAX", &[0x0f, 0x1f, 0xc0])
            .initial_register(Eax, 0x9234_5678),
        Case::preserving_flags(
            "word register form preserves all of ESP",
            &[0x66, 0x0f, 0x1f, 0xc4],
        )
        .initial_register(Esp, 0x8765_4321),
        Case::preserving_flags("address16 r/m=4 has no SIB", &[0x67, 0x0f, 0x1f, 0x04])
            .initial_register(Esi, 0x4000),
        Case::preserving_flags(
            "address16 consumes a complete displacement",
            &[0x67, 0x0f, 0x1f, 0x06, 0x8b, 0xc7],
        ),
        Case::preserving_flags(
            "complete encoding needs no following code page",
            &[0x0f, 0x1f, 0x44, 0x00, 0x00],
        )
        .at(0x1ffb)
        .initial_register(Eax, 0x4000),
    ]);
    for code in [
        &[0x0f, 0x1f, 0x04][..],
        &[0x67, 0x0f, 0x1f, 0x44, 0x8b, 0x80],
    ] {
        cases.push(
            Case::preserving_flags(format!("16-bit code defaults {code:02x?}"), code)
                .segmented_only()
                .segment(
                    Segment::Cs,
                    StoredSegment {
                        attributes: SegmentAttributes::from_bits(0x07),
                        ..StoredSegment::flat_code32(0x1b)
                    },
                ),
        );
    }
    cases
}

fn ignored_segments() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, segment, cache) in [
        (
            "unusable DS",
            &[0x0f, 0x1f, 0x03][..],
            Segment::Ds,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0),
                ..StoredSegment::flat_data32(0)
            },
        ),
        (
            "SS limit",
            &[0x0f, 0x1f, 0x04, 0x24],
            Segment::Ss,
            StoredSegment {
                limit: 0,
                ..StoredSegment::flat_data32(0x23)
            },
        ),
        (
            "unusable FS override",
            &[0x64, 0x0f, 0x1f, 0x03],
            Segment::Fs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0),
                ..StoredSegment::flat_data32(0)
            },
        ),
    ] {
        cases.push(
            Case::preserving_flags(name, code)
                .initial_registers(&[(Ebx, 0x4000), (Esp, 0x4000)])
                .segmented_only()
                .segment(segment, cache),
        );
    }
    cases
}

#[test]
fn complete_encodings_require_every_address_byte_and_no_successor() {
    for code in ALIGNMENT_NOPS.iter().copied().chain([
        &[0x0f, 0x1f, 0xc0][..],
        &[0x67, 0x0f, 0x1f, 0x04],
        &[0x67, 0x0f, 0x1f, 0x06, 0x8b, 0xc7],
    ]) {
        check_length(code);
    }
}

fn missing_encoding_bytes(engine: Engine) {
    for code in [
        &[0x0f, 0x1f][..],
        &[0x0f, 0x1f, 0x04],
        &[0x0f, 0x1f, 0x44, 0x00],
        &[0x0f, 0x1f, 0x84, 0x00, 0x78, 0x56, 0x34],
        &[0x67, 0x0f, 0x1f, 0x06, 0x78],
    ] {
        let mut image = Image::empty();
        image.cpu.eip = 0x2000 - code.len() as u32;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            &format!("unused address fields still need instruction bytes: {code:02x?}"),
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }
}

#[test]
fn interpreter_fetches_unused_modrm_sib_and_displacement_bytes() {
    missing_encoding_bytes(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interpreter_fetches_unused_modrm_sib_and_displacement_bytes() {
    missing_encoding_bytes(Engine::V8);
}

test_cases!(
    register_and_address_encodings_preserve_state,
    complete_forms()
);
test_cases!(
    nominal_addresses_do_not_access_data_segments,
    ignored_segments()
);

test_sequences!(
    pending_flags_and_later_fault,
    [Sequence::from_opaque_flags(
        "NOP preserves pending arithmetic and retires before a later fault"
    )
    .initial_registers(&[(Eax, 0), (Ebx, 0x4000), (Ecx, 0)])
    .step(
        Step::new(
            &[0x83, 0xe8, 1],
            Flags {
                cf: Set,
                pf: Set,
                af: Set,
                zf: Clear,
                sf: Set,
                of: Clear
            }
        )
        .register(Eax, u32::MAX)
    )
    .step(Step::preserving_flags(&[0x0f, 0x1f, 0x44, 0x00, 0x7f]))
    .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 1))
    .step(Step::preserving_flags(&[0x8a, 0x13]).fault(0x4000, 0))]
);
