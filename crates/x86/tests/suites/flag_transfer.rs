//! AH transfers cover each flag bit, fixed bits, and partial-flag interactions.
use crate::support::{
    cases::{
        test_cases,
        FlagExpectation::{self, Clear, Preserved, Set},
        Flags, InstructionCase as Case,
    },
    encoding::check_length,
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Sequence},
};
use wasm86_x86::{
    CpuState, FlagBytes,
    Gpr32::{Eax, Ebx, Ecx, Esi},
    StoredFlags, StoredStatusSource,
};

fn byte_flags(ah: u8, overflow: bool) -> Flags<bool> {
    Flags {
        cf: ah & 0x01 != 0,
        pf: ah & 0x04 != 0,
        af: ah & 0x10 != 0,
        zf: ah & 0x40 != 0,
        sf: ah & 0x80 != 0,
        of: overflow,
    }
}

fn sahf_flags(ah: u8) -> Flags<FlagExpectation> {
    let bit = |mask| if ah & mask != 0 { Set } else { Clear };
    Flags {
        cf: bit(0x01),
        pf: bit(0x04),
        af: bit(0x10),
        zf: bit(0x40),
        sf: bit(0x80),
        of: Preserved,
    }
}

fn concrete_record(flags: Flags<bool>, direction: u8) -> StoredFlags {
    StoredFlags {
        status_source: StoredStatusSource {
            kind: 0,
            reserved: [0x5a, 0xc3, 0x96],
            left: 0x1234_5678,
            right: 0x8765_4321,
        },
        bytes: FlagBytes {
            cf: 0x80 | u8::from(flags.cf),
            pf: 0x5a | u8::from(flags.pf),
            af: 0xfe | u8::from(flags.af),
            zf: 0xc2 | u8::from(flags.zf),
            sf: 0x3c | u8::from(flags.sf),
            of: 0x96 | u8::from(flags.of),
            df: direction,
            ..CpuState::filled(0xa5).flags.bytes
        },
    }
}

fn lahf_bits() -> Vec<Case> {
    // Zero, each participating bit, a mixed result, and all five bits set.
    [0x02, 0x03, 0x06, 0x12, 0x42, 0x82, 0x93, 0xd7]
        .into_iter()
        .enumerate()
        .map(|(index, ah)| {
            let flags = byte_flags(ah, index % 2 != 0);
            let code = if index == 0 {
                &[0x66, 0x9f][..]
            } else {
                &[0x9f][..]
            };
            Case::new(
                format!("LAHF produces AH={ah:02x} with fixed bits"),
                code,
                flags,
                Flags::all(Preserved),
            )
            .stored_flags(concrete_record(flags, 0xfe))
            .preserve_flag_record()
            .register(Eax, 0x4433_ff11, 0x4433_0011 | (u32::from(ah) << 8))
        })
        .collect()
}

fn sahf_bits() -> Vec<Case> {
    // Each source bit is isolated, including the three architecturally ignored bits.
    [0, 1, 4, 0x10, 0x40, 0x80, 2, 8, 0x20, 0xff]
        .into_iter()
        .enumerate()
        .map(|(index, ah)| {
            let flags = byte_flags(!ah, index % 2 != 0);
            let code = if index == 0 {
                &[0x66, 0x9e][..]
            } else {
                &[0x9e][..]
            };
            Case::new(
                format!("SAHF reads AH={ah:02x} and preserves OF"),
                code,
                flags,
                sahf_flags(ah),
            )
            .stored_flags(concrete_record(flags, 0x81))
            .initial_register(Eax, 0x4433_00a5 | (u32::from(ah) << 8))
        })
        .collect()
}

#[test]
fn transfers_consume_no_operand_byte_and_ignore_operand_size() {
    for code in [&[0x9e][..], &[0x9f], &[0x66, 0x9e], &[0x66, 0x9f]] {
        check_length(code);
    }
}

fn pending_flag_sequences() -> Vec<Sequence> {
    vec![
        Sequence::new(
            "LAHF reads pending byte ADD; SAHF preserves its overflow",
            Flags::all(false),
        )
        .initial_registers(&[
            (Eax, 0x4433_227f),
            (Ebx, 0x8877_6600),
            (Ecx, 0),
            (Esi, 0x6000),
        ])
        .step(
            Step::new(
                &[0x04, 1],
                Flags {
                    cf: Clear,
                    pf: Clear,
                    af: Set,
                    zf: Clear,
                    sf: Set,
                    of: Set,
                },
            )
            .register(Eax, 0x4433_2280),
        )
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9280))
        .step(Step::preserving_flags(&[0xb4, 0x6d]).register(Eax, 0x4433_6d80))
        .step(Step::new(&[0x9e], sahf_flags(0x6d)))
        .step(Step::preserving_flags(&[0x0f, 0x90, 0xc3]).register(Ebx, 0x8877_6601))
        .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 1))
        .step(Step::preserving_flags(&[0x66, 0x9f]).register(Eax, 0x4433_4780))
        .step(Step::preserving_flags(&[0x8b, 0x06]).fault(0x6000, 0))
        .trailing_code(&[0x9e, 0x9f], 2),
        Sequence::new(
            "LAHF reads preserved carry and INC flags; SAHF feeds a signed condition",
            Flags::all(false),
        )
        .initial_registers(&[(Eax, 0x7fff_ffff), (Ecx, 0)])
        .step(Step::new(
            &[0xf9],
            Flags {
                cf: Set,
                ..Flags::all(Preserved)
            },
        ))
        .step(
            Step::new(
                &[0x40],
                Flags {
                    cf: Preserved,
                    pf: Set,
                    af: Set,
                    zf: Clear,
                    sf: Set,
                    of: Set,
                },
            )
            .register(Eax, 0x8000_0000),
        )
        .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x8000_9700))
        .step(Step::preserving_flags(&[0xb4, 2]).register(Eax, 0x8000_0200))
        .step(Step::new(&[0x9e], sahf_flags(2)))
        .step(Step::preserving_flags(&[0x0f, 0x9e, 0xc1]).register(Ecx, 1)),
    ]
}

test_cases!(lahf_bit_positions_and_unchanged_backing, lahf_bits());
test_cases!(sahf_bit_positions_and_overflow_preservation, sahf_bits());
test_sequences!(
    pending_flags_and_fault_publication,
    pending_flag_sequences()
);
