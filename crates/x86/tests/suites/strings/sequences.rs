use super::{flags, record, Operation, OPERATIONS};
use crate::flags::Flag;
use crate::support::{
    cases::Permissions::{ReadOnly, ReadWrite},
    sequences::{test_sequences, Checkpoint as Step, SequenceCase as Case},
};
use wasm86_x86::Gpr32::{Eax, Ebx, Ecx, Edi, Esi};

fn histories() -> Vec<Case> {
    let mut cases = Vec::new();
    for (df_opcode, df, source, destination, next_source, next_destination) in [
        (0xfc, false, 0x4000, 0x6000, 0x4001, 0x6001),
        (0xfd, true, 0x4001, 0x6001, 0x4000, 0x6000),
    ] {
        cases.push(
            Case::from_opaque_flags(format!(
                "direction control feeds byte load/store and preserves pending ADD, DF {df}"
            ))
            .stored_flags(record(if df { 0xfe } else { 0xff }))
            .initial_registers(&[
                (Eax, 0x4433_227f),
                (Esi, source),
                (Edi, destination),
                (Ecx, 0),
            ])
            .memory(0x4000, &[0x12, 0x12], ReadOnly)
            .memory(0x6000, &[0xa5; 2], ReadWrite)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x4433_2280))
            .step(Step::preserving_flags(&[df_opcode]).expect_direct_flag(Flag::DF, df))
            .step(
                Step::preserving_flags(&[0xac])
                    .register(Esi, next_source)
                    .register(Eax, 0x4433_2212),
            )
            .step(
                Step::preserving_flags(&[0xaa])
                    .register(Edi, next_destination)
                    .expect_memory(destination, &[0x12]),
            )
            .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9212)),
        );
    }
    for operation in [Operation::Cmps, Operation::Scas] {
        cases.push(
            Case::from_opaque_flags(format!(
                "{operation:?} result feeds unsigned/signed SETcc, LAHF and ADC"
            ))
            .stored_flags(record(0xfe))
            .initial_registers(&[
                (Eax, 0x4433_2200),
                (Ebx, 0),
                (Ecx, 0xaabb_ccdd),
                (Esi, 0x4000),
                (Edi, 0x6000),
            ])
            .memory(0x4000, &[0], ReadOnly)
            .memory(0x6000, &[1], ReadOnly)
            .step(
                Step::new(&operation.code(1), flags(23))
                    .register(Edi, 0x6001)
                    .register(
                        Esi,
                        if operation.uses_source_index() {
                            0x4001
                        } else {
                            0x4000
                        },
                    ),
            )
            .step(Step::preserving_flags(&[0x0f, 0x92, 0xc1]).register(Ecx, 0xaabb_cc01))
            .step(Step::preserving_flags(&[0x0f, 0x9c, 0xc5]).register(Ecx, 0xaabb_0101))
            .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9700))
            .step(Step::new(&[0x83, 0xd3, 0], flags(0)).register(Ebx, 1)),
        );
    }
    cases.push(
        Case::from_opaque_flags("MOVSD preserves pending ADD until LAHF observes it")
            .stored_flags(record(0xfe))
            .initial_registers(&[(Eax, 0x4433_227f), (Esi, 0x4000), (Edi, 0x6000)])
            .memory(0x4000, &[1, 2, 3, 4], ReadOnly)
            .memory(0x6000, &[0xa5; 4], ReadWrite)
            .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x4433_2280))
            .step(
                Step::preserving_flags(&[0xa5])
                    .register(Esi, 0x4004)
                    .register(Edi, 0x6004)
                    .expect_memory(0x6000, &[1, 2, 3, 4]),
            )
            .step(Step::preserving_flags(&[0x9f]).register(Eax, 0x4433_9280)),
    );
    cases.push(
        Case::preserving_flags("completed MOVSD survives a later guest fault")
            .stored_flags(record(0xfe))
            .initial_registers(&[(Esi, 0x4000), (Edi, 0x6000), (Ebx, 0x9000)])
            .instruction_count(u32::MAX)
            .memory(0x4000, &[1, 2, 3, 4], ReadOnly)
            .memory(0x6000, &[0xa5; 4], ReadWrite)
            .step(
                Step::preserving_flags(&[0xa5])
                    .register(Esi, 0x4004)
                    .register(Edi, 0x6004)
                    .expect_memory(0x6000, &[1, 2, 3, 4]),
            )
            .step(Step::preserving_flags(&[0x8b, 0x03]).fault(0x9000, 0))
            .trailing_code(&[0xfd, 0xab], 2),
    );
    cases.push(
        Case::preserving_flags("an earlier guest fault prevents all following string operations")
            .stored_flags(record(0x81))
            .initial_registers(&[(Esi, 0x4000), (Edi, 0x6000), (Ebx, 0x9000)])
            .memory(0x4000, &[1, 2, 3, 4], ReadOnly)
            .memory(0x6000, &[0xa5; 4], ReadWrite)
            .step(Step::preserving_flags(&[0x8b, 0x03]).fault(0x9000, 0))
            .trailing_code(&[0xa5, 0xa7, 0xab, 0xad, 0xaf], 5),
    );
    for operation in OPERATIONS {
        let source = if operation == Operation::Lods {
            0x4fff
        } else {
            0x4000
        };
        let mut case = Case::from_opaque_flags(format!(
            "failed {operation:?} preserves earlier ADD and all indices"
        ))
        .stored_flags(record(0xff))
        .initial_registers(&[(Eax, 0x4433_227f), (Esi, source), (Edi, 0x7fff)])
        .map_page(4, 0x8000, ReadOnly)
        .backing(0x8000, &[1, 2])
        .backing(0x8fff, &[1])
        .step(Step::new(&[0x04, 1], flags(52)).register(Eax, 0x4433_2280));
        if operation.uses_destination_index() {
            case = case.map_page(7, 0xc000, ReadWrite).backing(0xcfff, &[0xa5]);
        }
        cases.push(
            case.step(Step::preserving_flags(&operation.code(2)).fault(
                if operation == Operation::Lods {
                    0x5000
                } else {
                    0x8000
                },
                if operation.writes_memory() { 2 } else { 0 },
            ))
            .trailing_code(&[0xfc, 0xa4], 2),
        );
    }
    cases
}

test_sequences!(direction_flags_pending_status_and_guest_faults, histories());
