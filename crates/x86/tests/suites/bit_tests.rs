//! Bit selection, old-bit carry and register versus memory index rules.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set, Undefined},
        Flags, InstructionCase as Case,
        Permissions::{ReadOnly, ReadWrite},
    },
    encoding::check_length,
    machine::{Exit, Image},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
    step::{Engine, TestModule},
};
use wasm86_x86::{compile_block_from_bytes, BlockError, CpuState, Gpr32::*, StoredStatusSource};

#[path = "bit_tests/memory.rs"]
mod memory;

const INITIAL_FLAGS: Flags<bool> = Flags {
    cf: false,
    pf: false,
    af: true,
    zf: true,
    sf: false,
    of: true,
};

// All four instructions preserve ZF; PF, AF, SF and OF are architecturally undefined.
fn bit_flags(carry: FlagExpectation) -> Flags<FlagExpectation> {
    Flags {
        cf: carry,
        pf: Undefined,
        af: Undefined,
        zf: Preserved,
        sf: Undefined,
        of: Undefined,
    }
}

fn immediate_results() -> Vec<Case> {
    [
        (0xe0, 0x8000_0001, 31, 0x8000_0001, Set),
        (0xe0, 0x8000_0001, 30, 0x8000_0001, Clear),
        (0xe8, 1, 0, 1, Set),
        (0xe8, 0, 31, 0x8000_0000, Clear),
        (0xf0, 0, 0, 0, Clear),
        (0xf0, u32::MAX, 31, 0x7fff_ffff, Set),
        (0xf8, 0, 0, 1, Clear),
        (0xf8, 1, 32, 0, Set),
    ]
    .into_iter()
    .map(|(modrm, input, index, result, carry)| {
        Case::new(
            format!("BA/{:x}: old bit {index} of {input:x}", (modrm >> 3) & 7),
            &[0x0f, 0xba, modrm, index],
            INITIAL_FLAGS,
            bit_flags(carry),
        )
        .register(Eax, input, result)
    })
    .collect()
}
test_cases!(old_bit_and_unchanged_writes, immediate_results());

fn register_indexes() -> Vec<Case> {
    let mut cases = Vec::new();
    // Literal carry columns distinguish masking to four or five index bits.
    for (index, word_carry, dword_carry) in [
        (0, Set, Set),
        (15, Set, Clear),
        (16, Set, Clear),
        (31, Set, Set),
        (32, Set, Set),
        (33, Clear, Clear),
        (0x1234_8000, Set, Set),
        (0xffff_0001, Clear, Clear),
        (0x8000_0000, Set, Set),
        (u32::MAX, Set, Set),
    ] {
        for (code, input, carry) in [
            (&[0x66, 0x0f, 0xa3, 0xd0][..], 0x4433_8001, word_carry),
            (&[0x0f, 0xa3, 0xd0][..], 0x8000_0001, dword_carry),
        ] {
            cases.push(
                Case::new(
                    format!("BT {code:02x?}, index {index:x}"),
                    code,
                    INITIAL_FLAGS,
                    bit_flags(carry),
                )
                .initial_registers(&[(Eax, input), (Edx, index)]),
            );
        }
    }
    cases
}
test_cases!(register_indexes_wrap_within_the_operand, register_indexes());

#[rustfmt::skip]
fn aliased_indexes() -> Vec<Case> {
    vec![
        Case::new("BT AX,AX uses the old low word", &[0x66, 0x0f, 0xa3, 0xc0], INITIAL_FLAGS, bit_flags(Clear))
            .initial_register(Eax, 0x4433_8003),
        Case::new("BTS AX,AX samples the index before setting it", &[0x66, 0x0f, 0xab, 0xc0], INITIAL_FLAGS, bit_flags(Clear))
            .register(Eax, 0x4433_8003, 0x4433_800b),
        Case::new("BTR ECX,ECX clears the old top bit", &[0x0f, 0xb3, 0xc9], INITIAL_FLAGS, bit_flags(Set))
            .register(Ecx, 0x8877_ffff, 0x0877_ffff),
        Case::new("BTC CX,CX preserves the upper word", &[0x66, 0x0f, 0xbb, 0xc9], INITIAL_FLAGS, bit_flags(Set))
            .register(Ecx, 0x8877_ffff, 0x8877_7fff),
    ]
}
test_cases!(indexes_capture_the_old_destination, aliased_indexes());

fn stored_arithmetic_flags() -> Vec<Case> {
    let mut record = CpuState::filled(0xa5).flags;
    record.status_source = StoredStatusSource {
        kind: 9,
        left: 0,
        right: 1,
        ..record.status_source
    };
    let initial = Flags {
        cf: true,
        pf: true,
        af: true,
        zf: false,
        sf: true,
        of: false,
    };
    // Preserve undefined flags by project policy, including a pending arithmetic source.
    // The state suite covers the complete stored-kind and materialization matrix.
    let expected = Flags {
        cf: Clear,
        pf: Preserved,
        af: Preserved,
        zf: Preserved,
        sf: Preserved,
        of: Preserved,
    };
    [
        (&[0x66, 0x0f, 0xa3, 0xd0][..], 1),
        (&[0x0f, 0xab, 0xd0][..], 3),
        (&[0x66, 0x0f, 0xb3, 0xd0][..], 1),
        (&[0x0f, 0xbb, 0xd0][..], 3),
    ]
    .into_iter()
    .map(|(code, result)| {
        Case::new(
            format!("{code:02x?} replaces only carry"),
            code,
            initial,
            expected,
        )
        .stored_flags(record)
        .register(Eax, 1, result)
        .initial_register(Edx, 1)
    })
    .collect()
}
test_cases!(
    carry_publication_preserves_other_pending_flags,
    stored_arithmetic_flags()
);

#[rustfmt::skip]
fn sequences() -> Vec<Sequence> {
    vec![
        Sequence::from_opaque_flags("BT replaces carry while retaining locally produced zero")
            .initial_registers(&[(Eax, 0x1234_56ff), (Ebx, 2), (Ecx, 32), (Edx, 0xccbb_aa99)])
            .step(Step::new(&[0x04, 1],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Eax, 0x1234_5600))
            .step(Step::new(&[0x0f, 0xa3, 0xcb], bit_flags(Clear)))
            .step(Step::preserving_flags(&[0x0f, 0x94, 0xc2]).register(Edx, 0xccbb_aa01))
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 0)),
        Sequence::new("changed indexes, memory bits and carry publish before a write fault", Flags::all(false))
            .initial_registers(&[(Ecx, 0x8877_ffff), (Ebx, 0x4000), (Esi, 0x5000), (Edi, u32::MAX)])
            .memory(0x4000, &[1, 0, 0, 0x80], ReadWrite).memory(0x5000, &[1, 0x80], ReadOnly)
            .step(Step::preserving_flags(&[0x66, 0xb9, 0x10, 0]).register(Ecx, 0x8877_0010))
            .step(Step::new(&[0x66, 0x0f, 0xbb, 0xc9], bit_flags(Clear)).register(Ecx, 0x8877_0011))
            .step(Step::new(&[0x0f, 0xba, 0x33, 31], bit_flags(Set)).expect_memory(0x4000, &[1, 0, 0, 0]))
            .step(Step::new(&[0x83, 0xd7, 0],
                Flags { cf: Set, pf: Set, af: Set, zf: Set, sf: Clear, of: Clear }).register(Edi, 0))
            .step(Step::preserving_flags(&[0x66, 0x0f, 0xba, 0x2e, 255]).fault(0x5000, 3)),
    ]
}
test_sequences!(local_flags_and_memory_progress, sequences());

#[test]
fn forms_decode_the_complete_address_and_index() {
    for (opcode, group) in [(0xa3, 0x20), (0xab, 0x28), (0xb3, 0x30), (0xbb, 0x38)] {
        for code in [
            vec![0x0f, opcode, 0xd0],
            vec![0x0f, 0xba, 0xc0 | group, 255],
            vec![0x66, 0x0f, opcode, 0x54, 0x8b, 0xfc],
            vec![0x66, 0x0f, 0xba, 0x44 | group, 0x8b, 0xfc, 255],
        ] {
            check_length(&code);
        }
    }
}

#[test]
fn unsupported_ba_extensions_precede_address_or_immediate_fetches() {
    for extension in 0..4 {
        let code = [0x0f, 0xba, 0x04 | (extension << 3)];
        assert_eq!(
            compile_block_from_bytes(0x1ffd, &code, 1).err(),
            Some(BlockError::UnsupportedInstruction {
                address: 0x1ffd,
                opcode: 0x0f
            })
        );
        let mut image = Image::new(&[]);
        image.cpu.eip = 0x1ffd;
        image.data(0x3ffd, &code);
        image.check_unchanged_exit(
            Engine::Wasmtime,
            TestModule::interpreter(),
            "unsupported BA extension needs no further bytes",
            Exit::Other(0x0008_000f_0000_1ffd),
        );
    }
}
