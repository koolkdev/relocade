use super::{EVEN, ODD, ZERO};
use crate::support::cases::{test_cases, InstructionCase as Case, RegisterExpectation::Exact};
use wasm86_x86::{CpuState, Gpr32, StoredFlags};
use wasm86_x86::{FlagBytes, StoredStatusSource};

fn parity_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, source, first, last, flags) in [
        (&[0x66][..], 0, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[][..], 0, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[0x66][..], 0x100, 0x4433_0008, 0x4433_0008, ODD),
        (&[][..], 0x100, 8, 8, ODD),
        (&[0x66][..], 0x101, 0x4433_0000, 0x4433_0008, EVEN),
        (&[][..], 0x101, 0, 8, EVEN),
        (&[0x66][..], 0x1_0000, 0x4433_a55b, 0x4433_a55b, ZERO),
        (&[][..], 0x1_0000, 16, 16, ODD),
        (&[0x66][..], 0x1_0001, 0x4433_0000, 0x4433_0000, ODD),
        (&[][..], 0x1_0001, 0, 16, EVEN),
        (&[0x66][..], 0x1_0100, 0x4433_0008, 0x4433_0008, ODD),
        (&[][..], 0x1_0100, 8, 16, EVEN),
    ] {
        for (opcode, result) in [(0xbc, first), (0xbd, last)] {
            cases.push(Case::replacing_flags(format!("full source parity, prefix {prefix:02x?}, opcode {opcode:x}, source {source:x}"),
                &[prefix, &[0x0f, opcode, 0xc2]].concat(), flags)
                .register(Gpr32::Eax, 0x4433_a55b, result).initial_register(Gpr32::Edx, source));
        }
    }
    cases
}
test_cases!(full_source_parity, parity_cases());

fn alias_cases() -> Vec<Case> {
    let registers = [
        Gpr32::Eax,
        Gpr32::Ecx,
        Gpr32::Edx,
        Gpr32::Ebx,
        Gpr32::Esp,
        Gpr32::Ebp,
        Gpr32::Esi,
        Gpr32::Edi,
    ];
    let mut cases = Vec::new();
    // Results are full parent registers: [BSF distinct, BSR distinct, BSF self, BSR self].
    for (prefix, source, results, flags) in [
        (&[0x66][..], 0, [0x4433_a55b, 0x4433_a55b, 0, 0], ZERO),
        (
            &[0x66][..],
            0xffff_0000,
            [0x4433_a55b, 0x4433_a55b, 0xffff_0000, 0xffff_0000],
            ZERO,
        ),
        (
            &[0x66][..],
            0x8001_0080,
            [0x4433_0007, 0x4433_0007, 0x8001_0007, 0x8001_0007],
            ODD,
        ),
        (&[][..], 0, [0x4433_a55b, 0x4433_a55b, 0, 0], ZERO),
        (&[][..], 0xffff_0000, [16, 31, 16, 31], EVEN),
        (&[][..], 0x8001_0080, [7, 31, 7, 31], ODD),
    ] {
        for (destination_code, &destination) in registers.iter().enumerate() {
            for self_source in [false, true] {
                let source_code = if self_source {
                    destination_code
                } else {
                    (destination_code + 3) % registers.len()
                };
                for (operation, opcode) in [0xbc, 0xbd].into_iter().enumerate() {
                    let code = [
                        prefix,
                        &[
                            0x0f,
                            opcode,
                            0xc0 | ((destination_code as u8) << 3) | source_code as u8,
                        ],
                    ]
                    .concat();
                    let output = results[operation + if self_source { 2 } else { 0 }];
                    let mut case = Case::replacing_flags(
                        format!("{code:02x?}, source {source:x}"),
                        &code,
                        flags,
                    )
                    .initial_register(registers[source_code], source)
                    .expect_register(destination, Exact(output));
                    if !self_source {
                        case = case.initial_register(destination, 0x4433_a55b);
                    }
                    cases.push(case);
                }
            }
        }
    }
    cases
}
test_cases!(all_destination_and_self_source_encodings, alias_cases());

fn flag_replacement_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    for kind in [0, 1, 5, 9, 2, 6, 10, 3, 7, 11] {
        for (prefix, source, first, last, flags) in [
            (&[0x66][..], 0, 0x4433_a55b, 0x4433_a55b, ZERO),
            (&[][..], 0, 0x4433_a55b, 0x4433_a55b, ZERO),
            (&[0x66][..], 0x100, 0x4433_0008, 0x4433_0008, ODD),
            (&[][..], 0x100, 8, 8, ODD),
            (&[0x66][..], 0x101, 0x4433_0000, 0x4433_0008, EVEN),
            (&[][..], 0x101, 0, 8, EVEN),
        ] {
            for (opcode, result) in [(0xbc, first), (0xbd, last)] {
                cases.push(Case::replacing_flags(format!("opcode {opcode:x}, prefix {prefix:02x?}, source {source:x} replaces kind {kind}"),
                    &[prefix, &[0x0f, opcode, 0xc2]].concat(), flags)
                    .stored_flags(StoredFlags {
                        status_source: StoredStatusSource {
                            kind,
                            left: 0x1234_5678,
                            right: 0x8765_4321,
                            ..(CpuState::filled(0xa5).flags).status_source
                        },
                        bytes: FlagBytes {
                            cf: 1,
                            pf: 1,
                            af: 1,
                            zf: 1,
                            sf: 1,
                            of: 1,
                            ..(CpuState::filled(0xa5).flags).bytes
                        },
                    })
                    .register(Gpr32::Eax, 0x4433_a55b, result).initial_register(Gpr32::Edx, source));
            }
        }
    }
    cases
}
test_cases!(every_stored_flag_kind_is_replaced, flag_replacement_cases());
