//! UD2 raises a guest fault after its encoding is fetched and before any retirement.

use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{Clear, Set},
        Flags, InstructionCase as Case,
        Permissions::ReadWrite,
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{
    compile_block_from_bytes, Gpr32::*, Segment, SegmentAttributes, SegmentProfile, StoredSegment,
};

fn faulting_forms() -> Vec<Case> {
    vec![
        Case::preserving_flags(
            "UD2 preserves all registers, flags and memory",
            &[0x0f, 0x0b],
        )
        .memory(0x4000, &[0x12, 0x34, 0x56, 0x78], ReadWrite)
        .invalid_opcode(),
        Case::preserving_flags(
            "size and segment prefixes keep the restart EIP at the first prefix",
            &[0x66, 0x67, 0x64, 0x0f, 0x0b],
        )
        .segment(Segment::Fs, StoredSegment::unusable(0))
        .invalid_opcode(),
        Case::preserving_flags("a complete UD2 needs no following code page", &[0x0f, 0x0b])
            .at(0x1ffe)
            .invalid_opcode(),
        Case::preserving_flags(
            "UD2 faults in 16-bit code with an operand override",
            &[0x66, 0x0f, 0x0b],
        )
        .segmented_only()
        .segment(
            Segment::Cs,
            StoredSegment {
                attributes: SegmentAttributes::from_bits(0x07),
                ..StoredSegment::flat_code32(0x1b)
            },
        )
        .invalid_opcode(),
    ]
}

#[test]
fn complete_encoding_ends_snapshot_compilation() {
    for code in [&[0x0f, 0x0b][..], &[0x66, 0x67, 0x64, 0x0f, 0x0b]] {
        let complete = check_length(code);
        for suffix in [&[][..], &[0x0f], &[0xf4], &[0xb0, 0x7f]] {
            assert_eq!(
                compile_block_from_bytes(0x1000, &[code, suffix].concat(), u32::MAX)
                    .unwrap()
                    .bytes,
                complete.bytes,
                "UD2 stops before suffix {suffix:02x?}",
            );
        }
    }
}

fn instruction_fetch_faults(engine: Engine) {
    for code in [&[0x0f][..], &[0x66, 0x0f]] {
        let mut image = Image::empty();
        image.cpu.eip = 0x2000 - code.len() as u32;
        image.map(1, 0x3000, false);
        image.data(0x4000 - code.len() as u32, code);
        image.check_unchanged_exit(
            engine,
            TestModule::interpreter(),
            "the missing second opcode byte faults before UD2 can execute",
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        );
    }

    let mut image = Image::new(&[0x0f, 0x0b]);
    image.cpu.segments.cs.limit = 0x1000;
    image.check_unchanged_exit(
        engine,
        TestModule::interpreter_with_profile(SegmentProfile::Segmented32),
        "the second opcode byte must be inside CS before UD2 can execute",
        Exit::GeneralProtection { error: 0 },
    );
}

#[test]
fn interpreter_fetch_checks_precede_the_invalid_opcode_fault() {
    instruction_fetch_faults(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interpreter_fetch_checks_precede_the_invalid_opcode_fault() {
    instruction_fetch_faults(Engine::V8);
}

test_cases!(fault_preserves_instruction_entry, faulting_forms());

test_sequences!(
    completed_work_survives_the_fault,
    [
        Sequence::from_opaque_flags("UD2 publishes earlier arithmetic and stores, then stops")
            .initial_registers(&[(Eax, 0x7fff_ffff), (Ebx, 0x4000)])
            .memory(0x4000, &[0; 4], ReadWrite)
            .step(
                Step::new(
                    &[0x83, 0xc0, 1],
                    Flags {
                        cf: Clear,
                        pf: Set,
                        af: Set,
                        zf: Clear,
                        sf: Set,
                        of: Set
                    },
                )
                .register(Eax, 0x8000_0000),
            )
            .step(Step::preserving_flags(&[0x89, 0x03]).expect_memory(0x4000, &[0, 0, 0, 0x80]))
            .step(Step::preserving_flags(&[0x0f, 0x0b]).invalid_opcode())
            .trailing_code(&[0xb0, 0x77], 1)
    ]
);
