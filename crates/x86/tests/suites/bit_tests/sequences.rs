use wasm86_x86::Gpr32::{Eax, Ebp, Ebx, Ecx, Edi, Edx, Esi, Esp};

use crate::support::{
    cases::{
        FlagExpectation,
        FlagExpectation::{Clear, Preserved, Set},
        Flags,
        Permissions::{ReadOnly, ReadWrite},
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};

use super::bit_flags;

#[derive(Clone, Copy)]
struct CarryPath {
    carry: FlagExpectation,
    edx_after_setc: u32,
    edi_after_adc: u32,
    flags_after_adc: Flags<FlagExpectation>,
    eax_after_bts: u32,
    bts_carry: FlagExpectation,
}

// These are literal outcomes for the two carry values read by SETC and ADC.
const CARRY_SET: CarryPath = CarryPath {
    carry: Set,
    edx_after_setc: 0xccbb_aa01,
    edi_after_adc: 0xdead_0000,
    flags_after_adc: Flags {
        cf: Set,
        pf: Set,
        af: Set,
        zf: Set,
        sf: Clear,
        of: Clear,
    },
    eax_after_bts: 0x4433_8003,
    bts_carry: Clear,
};
const CARRY_CLEAR: CarryPath = CarryPath {
    carry: Clear,
    edx_after_setc: 0xccbb_aa00,
    edi_after_adc: 0xdead_ffff,
    flags_after_adc: Flags {
        cf: Clear,
        pf: Set,
        af: Clear,
        zf: Clear,
        sf: Set,
        of: Clear,
    },
    eax_after_bts: 0x4433_8001,
    bts_carry: Set,
};

fn pending_flags_and_carry_before_a_fault() -> Vec<SequenceCase> {
    struct Values {
        index: u32,
        initial_ecx: u32,
        path: CarryPath,
        ecx_after_btc: u32,
        btc_carry: FlagExpectation,
        edx_after_setc_dh: u32,
        ebp_after_adc: u32,
    }
    let mut cases = Vec::new();
    for values in [
        Values {
            index: 0x0000_0000,
            initial_ecx: 0x8877_0000,
            path: CARRY_SET,
            ecx_after_btc: 0x8877_0001,
            btc_carry: Clear,
            edx_after_setc_dh: 0xccbb_0001,
            ebp_after_adc: 0x8000_0001,
        },
        Values {
            index: 0x0000_0001,
            initial_ecx: 0x8877_0001,
            path: CARRY_CLEAR,
            ecx_after_btc: 0x8877_0003,
            btc_carry: Clear,
            edx_after_setc_dh: 0xccbb_0000,
            ebp_after_adc: 0x8000_0001,
        },
        Values {
            index: 0x0000_000f,
            initial_ecx: 0x8877_000f,
            path: CARRY_CLEAR,
            ecx_after_btc: 0x8877_800f,
            btc_carry: Clear,
            edx_after_setc_dh: 0xccbb_0000,
            ebp_after_adc: 0x8000_0001,
        },
        Values {
            index: 0x0000_0010,
            initial_ecx: 0x8877_0010,
            path: CARRY_CLEAR,
            ecx_after_btc: 0x8877_0011,
            btc_carry: Clear,
            edx_after_setc_dh: 0xccbb_0000,
            ebp_after_adc: 0x8000_0001,
        },
        Values {
            index: 0x0000_001f,
            initial_ecx: 0x8877_001f,
            path: CARRY_SET,
            ecx_after_btc: 0x8877_801f,
            btc_carry: Clear,
            edx_after_setc_dh: 0xccbb_0001,
            ebp_after_adc: 0x8000_0001,
        },
        Values {
            index: 0x0000_0020,
            initial_ecx: 0x8877_0020,
            path: CARRY_SET,
            ecx_after_btc: 0x8877_0021,
            btc_carry: Clear,
            edx_after_setc_dh: 0xccbb_0001,
            ebp_after_adc: 0x8000_0001,
        },
        Values {
            index: 0x0000_0021,
            initial_ecx: 0x8877_0021,
            path: CARRY_CLEAR,
            ecx_after_btc: 0x8877_0023,
            btc_carry: Clear,
            edx_after_setc_dh: 0xccbb_0000,
            ebp_after_adc: 0x8000_0001,
        },
        Values {
            index: 0x0000_ffff,
            initial_ecx: 0x8877_ffff,
            path: CARRY_SET,
            ecx_after_btc: 0x8877_7fff,
            btc_carry: Set,
            edx_after_setc_dh: 0xccbb_0101,
            ebp_after_adc: 0x8000_0002,
        },
        Values {
            index: 0xffff_ffff,
            initial_ecx: 0xffff_ffff,
            path: CARRY_SET,
            ecx_after_btc: 0xffff_7fff,
            btc_carry: Set,
            edx_after_setc_dh: 0xccbb_0101,
            ebp_after_adc: 0x8000_0002,
        },
    ] {
        let path = values.path;
        cases.push(
            SequenceCase::from_opaque_flags(format!(
                "bit index {:x} feeds conditions, partial flags and ADC before a write fault",
                values.index,
            ))
            .stored_flags(super::STORED_FLAGS)
            .instruction_count(0xffff_fff9)
            .initial_registers(&[
                (Eax, 0x4433_80ff),
                (Ecx, values.initial_ecx),
                (Edx, 0xccbb_aa01),
                (Ebx, 0x5002),
                (Esp, 0x4000),
                (Ebp, 0x8000_0001),
                (Esi, u32::MAX),
                (Edi, 0xdead_ffff),
            ])
            .map_page(4, 0x8000, ReadWrite)
            .map_page(5, 0xa000, ReadOnly)
            .backing(0x7fff, &[0x5a, 1, 0, 0, 0x80, 0x5a])
            .backing(0x9fff, &[0x5a, 0, 0x80, 1, 0, 0x5a])
            // ADD AL,DL publishes zero, carry and auxiliary carry.
            .step(
                Checkpoint::new(
                    &[0x00, 0xd0],
                    Flags {
                        cf: Set,
                        pf: Set,
                        af: Set,
                        zf: Set,
                        sf: Clear,
                        of: Clear,
                    },
                )
                .register(Eax, 0x4433_8000),
            )
            .step(Checkpoint::new(&[0x0f, 0xa3, 0xcd], bit_flags(path.carry))) // BT EBP,ECX
            .step(
                Checkpoint::preserving_flags(&[0x0f, 0x94, 0xc0]) // SETZ AL
                    .register(Eax, 0x4433_8001),
            )
            .step(
                Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc2]) // SETC DL
                    .register(Edx, path.edx_after_setc),
            )
            .step(
                Checkpoint::new(
                    &[0x46],
                    Flags {
                        // INC ESI preserves carry.
                        cf: Preserved,
                        pf: Set,
                        af: Set,
                        zf: Set,
                        sf: Clear,
                        of: Clear,
                    },
                )
                .register(Esi, 0),
            )
            .step(
                Checkpoint::new(&[0x66, 0x83, 0xd7, 0], path.flags_after_adc) // ADC DI,0
                    .register(Edi, path.edi_after_adc),
            )
            .step(
                Checkpoint::new(&[0x66, 0x0f, 0xab, 0xd0], bit_flags(path.bts_carry)) // BTS AX,DX
                    .register(Eax, path.eax_after_bts),
            )
            .step(
                Checkpoint::new(&[0x0f, 0xba, 0x34, 0x24, 31], bit_flags(Set)) // BTR [ESP],31
                    .expect_memory(0x4000, &[1, 0, 0, 0]),
            )
            .step(
                Checkpoint::new(&[0x66, 0x0f, 0xbb, 0xc9], bit_flags(values.btc_carry)) // BTC CX,CX
                    .register(Ecx, values.ecx_after_btc),
            )
            .step(
                Checkpoint::preserving_flags(&[0x0f, 0x92, 0xc6]) // SETC DH
                    .register(Edx, values.edx_after_setc_dh),
            )
            .step(
                Checkpoint::new(
                    &[0x83, 0xd5, 0],
                    Flags {
                        // ADC EBP,0
                        cf: Clear,
                        pf: Clear,
                        af: Clear,
                        zf: Clear,
                        sf: Set,
                        of: Clear,
                    },
                )
                .register(Ebp, values.ebp_after_adc),
            )
            // DI is -1 or zero; both indexed read-only words have their tested bit set.
            .step(Checkpoint::new(&[0x66, 0x0f, 0xa3, 0x3b], bit_flags(Set)))
            .step(Checkpoint::preserving_flags(&[0x66, 0x0f, 0xba, 0x2b, 255]).fault(0x5002, 3)),
        );
    }
    cases
}

test_sequences!(
    pending_flags_conditions_and_carry_before_fault,
    pending_flags_and_carry_before_a_fault()
);
