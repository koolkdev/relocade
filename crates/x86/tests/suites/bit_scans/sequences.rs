use super::{EVEN, ODD, ZERO};
use crate::support::{
    cases::{
        FlagExpectation,
        FlagExpectation::{Clear, Preserved, Set, Undefined},
        Flags,
        Permissions::ReadOnly,
    },
    sequences::{test_sequences, Checkpoint, SequenceCase},
};
use wasm86_x86::Gpr32;

struct ScanResults {
    source: u32,
    eax_after_word_scan: u32,
    word_flags: Flags<FlagExpectation>,
    eax_after_parity: u32,
    ecx_after_zero: u32,
    ecx_after_nonzero: u32,
    edi_after_dword_scan: u32,
    dword_flags: Flags<FlagExpectation>,
    ebx_after_parity: u32,
    ebx_after_zero: u32,
    eax_after_final_nonzero: u32,
}

fn scan_sequences() -> Vec<SequenceCase> {
    // Literal checkpoints distinguish low-word scans, full-source parity,
    // zero destination preservation, and later byte writes to that destination.
    [
        ScanResults {
            source: 0,
            eax_after_word_scan: 0x4433_7c5b,
            word_flags: ZERO,
            eax_after_parity: 0x4433_015b,
            ecx_after_zero: 0x8877_6601,
            ecx_after_nonzero: 0x8877_0001,
            edi_after_dword_scan: 0x1122_a55b,
            dword_flags: ZERO,
            ebx_after_parity: 0xccbb_0100,
            ebx_after_zero: 0xccbb_0101,
            eax_after_final_nonzero: 0x4433_0101,
        },
        ScanResults {
            source: 1,
            eax_after_word_scan: 0x4433_0000,
            word_flags: ODD,
            eax_after_parity: 0x4433_0000,
            ecx_after_zero: 0x8877_6600,
            ecx_after_nonzero: 0x8877_0100,
            edi_after_dword_scan: 0,
            dword_flags: ODD,
            ebx_after_parity: 0xccbb_0000,
            ebx_after_zero: 0xccbb_0000,
            eax_after_final_nonzero: 0x4433_0001,
        },
        ScanResults {
            source: 0x100,
            eax_after_word_scan: 0x4433_0008,
            word_flags: ODD,
            eax_after_parity: 0x4433_0008,
            ecx_after_zero: 0x8877_6600,
            ecx_after_nonzero: 0x8877_0100,
            edi_after_dword_scan: 8,
            dword_flags: ODD,
            ebx_after_parity: 0xccbb_0000,
            ebx_after_zero: 0xccbb_0000,
            eax_after_final_nonzero: 0x4433_0001,
        },
        ScanResults {
            source: 0x8000,
            eax_after_word_scan: 0x4433_000f,
            word_flags: ODD,
            eax_after_parity: 0x4433_000f,
            ecx_after_zero: 0x8877_6600,
            ecx_after_nonzero: 0x8877_0100,
            edi_after_dword_scan: 15,
            dword_flags: ODD,
            ebx_after_parity: 0xccbb_0000,
            ebx_after_zero: 0xccbb_0000,
            eax_after_final_nonzero: 0x4433_0001,
        },
        ScanResults {
            source: 0xffff_0000,
            eax_after_word_scan: 0x4433_7c5b,
            word_flags: ZERO,
            eax_after_parity: 0x4433_015b,
            ecx_after_zero: 0x8877_6601,
            ecx_after_nonzero: 0x8877_0001,
            edi_after_dword_scan: 31,
            dword_flags: EVEN,
            ebx_after_parity: 0xccbb_0100,
            ebx_after_zero: 0xccbb_0100,
            eax_after_final_nonzero: 0x4433_0101,
        },
        ScanResults {
            source: 0x8000_0000,
            eax_after_word_scan: 0x4433_7c5b,
            word_flags: ZERO,
            eax_after_parity: 0x4433_015b,
            ecx_after_zero: 0x8877_6601,
            ecx_after_nonzero: 0x8877_0001,
            edi_after_dword_scan: 31,
            dword_flags: ODD,
            ebx_after_parity: 0xccbb_0000,
            ebx_after_zero: 0xccbb_0000,
            eax_after_final_nonzero: 0x4433_0101,
        },
        ScanResults {
            source: 0x8008,
            eax_after_word_scan: 0x4433_0003,
            word_flags: EVEN,
            eax_after_parity: 0x4433_0103,
            ecx_after_zero: 0x8877_6600,
            ecx_after_nonzero: 0x8877_0100,
            edi_after_dword_scan: 15,
            dword_flags: EVEN,
            ebx_after_parity: 0xccbb_0100,
            ebx_after_zero: 0xccbb_0100,
            eax_after_final_nonzero: 0x4433_0101,
        },
        ScanResults {
            source: 0x8000_0001,
            eax_after_word_scan: 0x4433_0000,
            word_flags: ODD,
            eax_after_parity: 0x4433_0000,
            ecx_after_zero: 0x8877_6600,
            ecx_after_nonzero: 0x8877_0100,
            edi_after_dword_scan: 31,
            dword_flags: EVEN,
            ebx_after_parity: 0xccbb_0100,
            ebx_after_zero: 0xccbb_0100,
            eax_after_final_nonzero: 0x4433_0001,
        },
    ]
    .into_iter()
    .map(|result| {
        let source = result.source;
        SequenceCase::from_opaque_flags(format!(
            "scan source {source:08x}, aliases and flags before a fault"
        ))
        .instruction_count(0xffff_fff5)
        .initial_registers(&[
            (Gpr32::Eax, 0x4433_a55b),
            (Gpr32::Ebx, 0xccbb_00ff),
            (Gpr32::Ecx, 0x8877_6655),
            (Gpr32::Edx, source),
            (Gpr32::Edi, 0x1122_a55b),
            (Gpr32::Esi, 0x5566_a55b),
            (Gpr32::Esp, 0x4000),
            (Gpr32::Ebp, 0x5001),
        ])
        .map_page(4, 0x8000, ReadOnly)
        .backing(0x7fff, &[0x5a, 0, 0, 0x5a])
        .step(
            Checkpoint::preserving_flags(&[0xb8, 0x5b, 0xa5, 0x33, 0x44])
                .register(Gpr32::Eax, 0x4433_a55b),
        )
        .step(Checkpoint::preserving_flags(&[0xb4, 0x7c]).register(Gpr32::Eax, 0x4433_7c5b))
        .step(
            Checkpoint::new(
                &[0x80, 0xc3, 1],
                Flags {
                    cf: Set,
                    pf: Set,
                    af: Set,
                    zf: Set,
                    sf: Clear,
                    of: Clear,
                },
            )
            .register(Gpr32::Ebx, 0xccbb_0000),
        )
        .step(
            Checkpoint::new(&[0x66, 0x0f, 0xbc, 0xc2], result.word_flags)
                .register(Gpr32::Eax, result.eax_after_word_scan),
        )
        .step(
            Checkpoint::preserving_flags(&[0x0f, 0x9a, 0xc4])
                .register(Gpr32::Eax, result.eax_after_parity),
        )
        .step(
            Checkpoint::preserving_flags(&[0x0f, 0x94, 0xc1])
                .register(Gpr32::Ecx, result.ecx_after_zero),
        )
        .step(
            Checkpoint::preserving_flags(&[0x0f, 0x95, 0xc5])
                .register(Gpr32::Ecx, result.ecx_after_nonzero),
        )
        .step(Checkpoint::new(
            &[0x0f, 0xba, 0xed, 0],
            Flags {
                cf: Set,
                zf: Preserved,
                ..Flags::all(Undefined)
            },
        ))
        .step(
            Checkpoint::new(&[0x0f, 0xbd, 0xfa], result.dword_flags)
                .register(Gpr32::Edi, result.edi_after_dword_scan),
        )
        .step(
            Checkpoint::preserving_flags(&[0x0f, 0x9a, 0xc7])
                .register(Gpr32::Ebx, result.ebx_after_parity),
        )
        .step(
            Checkpoint::preserving_flags(&[0x0f, 0x94, 0xc3])
                .register(Gpr32::Ebx, result.ebx_after_zero),
        )
        .step(Checkpoint::preserving_flags(&[0xba, 0, 0, 0, 0]).register(Gpr32::Edx, 0))
        .step(Checkpoint::new(&[0x0f, 0xbc, 0xc2], ZERO))
        .step(Checkpoint::preserving_flags(&[0x0f, 0x94, 0xc6]).register(Gpr32::Edx, 0x100))
        .step(Checkpoint::new(&[0x66, 0x0f, 0xbd, 0xd2], ODD).register(Gpr32::Edx, 8))
        .step(
            Checkpoint::preserving_flags(&[0x0f, 0x95, 0xc0])
                .register(Gpr32::Eax, result.eax_after_final_nonzero),
        )
        .step(Checkpoint::new(&[0x66, 0x0f, 0xbc, 0x34, 0x24], ZERO))
        .step(Checkpoint::preserving_flags(&[0x0f, 0xbd, 0x6d, 0]).fault(0x5001, 0))
    })
    .collect()
}

test_sequences!(aliases_conditions_and_fault_publication, scan_sequences());
