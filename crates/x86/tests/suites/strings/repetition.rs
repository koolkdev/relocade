//! REP MOVS/STOS retain element ordering and restartable architectural progress.
use super::{record, Operation};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{Eax, Ecx, Edi, Esi};

fn rep(operation: Operation, width: u32) -> Vec<u8> {
    [vec![0xf3], operation.code(width)].concat()
}

fn zero_count() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in [Operation::Movs, Operation::Stos] {
        for width in [1, 2, 4] {
            for df in [0xfe, 0xff] {
                // Neither an unmapped operand nor a wrapping wide span is examined.
                cases.push(
                    Case::preserving_flags(
                        format!("REP {operation:?} width {width} zero ECX DF {df:02x}"),
                        &rep(operation, width),
                    )
                    .stored_flags(record(df))
                    .instruction_count(u32::MAX)
                    .initial_registers(&[
                        (Ecx, 0),
                        (Esi, u32::MAX),
                        (Edi, u32::MAX),
                        (Eax, 0x8765_4321),
                    ]),
                );
            }
        }
    }
    cases
}

fn transfers() -> Vec<Case> {
    let mut cases = Vec::new();
    let source = [
        0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x21, 0x43, 0x65, 0x87,
    ];
    for operation in [Operation::Movs, Operation::Stos] {
        for width in [1, 2, 4] {
            for count in [1, 3] {
                for backward in [false, true] {
                    let offset = if backward { width * (count - 1) } else { 0 };
                    let final_source = if backward {
                        0x4000u32.wrapping_sub(width)
                    } else {
                        0x4000 + width * count
                    };
                    let final_destination = if backward {
                        0x7000u32.wrapping_sub(width)
                    } else {
                        0x7000 + width * count
                    };
                    let mut case = Case::preserving_flags(
                        format!(
                            "REP {operation:?} width {width}, {count} iterations, backward {backward}"
                        ),
                        &rep(operation, width),
                    )
                    .stored_flags(record(if backward { 0xff } else { 0xfe }))
                    .register(Ecx, count, 0)
                    .memory(0x4000, &source, ReadOnly)
                    .memory(0x7000, &[0xa5; 12], ReadWrite);
                    if operation.uses_source_index() {
                        case = case.register(Esi, 0x4000 + offset, final_source);
                    } else {
                        case = case.initial_register(Esi, 0x4000 + offset);
                    }
                    if operation.uses_destination_index() {
                        case = case.register(Edi, 0x7000 + offset, final_destination);
                    } else {
                        case = case.initial_register(Edi, 0x7000 + offset);
                    }
                    case = case.initial_register(Eax, 0x7856_3412);
                    let bytes = if operation == Operation::Movs {
                        source[..(width * count) as usize].to_vec()
                    } else {
                        [0x12, 0x34, 0x56, 0x78][..width as usize].repeat(count as usize)
                    };
                    case = case.expect_memory(0x7000, &bytes);
                    cases.push(case);
                }
            }
        }
    }
    cases
}

fn final_index_wrap() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in [Operation::Movs, Operation::Stos] {
        for width in [1, 2, 4] {
            for backward in [false, true] {
                let (destination, final_destination) = if backward {
                    (0, 0u32.wrapping_sub(width))
                } else {
                    (0u32.wrapping_sub(width), 0)
                };
                let payload = &[0x12, 0x34, 0x56, 0x78][..width as usize];
                let mut case = Case::preserving_flags(
                    format!(
                        "REP {operation:?} width {width} final index wrap, backward {backward}"
                    ),
                    &rep(operation, width),
                )
                .stored_flags(record(if backward { 0xff } else { 0xfe }))
                .instruction_count(u32::MAX)
                .register(Ecx, 1, 0)
                .register(Edi, destination, final_destination)
                .initial_register(Eax, 0x7856_3412)
                .memory(destination, &vec![0xa5; width as usize], ReadWrite)
                .expect_memory(destination, payload);
                if operation == Operation::Movs {
                    let final_source = if backward {
                        0x4000 - width
                    } else {
                        0x4000 + width
                    };
                    case = case
                        .register(Esi, 0x4000, final_source)
                        .memory(0x4000, payload, ReadOnly);
                } else {
                    case = case.initial_register(Esi, u32::MAX);
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn overlap() -> Vec<Case> {
    vec![
        Case::preserving_flags(
            "REP MOVSB forward overlap propagates the prior write",
            &[0xf3, 0xa4],
        )
        .stored_flags(record(0xfe))
        .register(Ecx, 4, 0)
        .register(Esi, 0x4000, 0x4004)
        .register(Edi, 0x4001, 0x4005)
        .memory(0x4000, &[1, 2, 3, 4, 5, 6], ReadWrite)
        .expect_memory(0x4000, &[1, 1, 1, 1, 1, 6]),
        Case::preserving_flags(
            "REP MOVSB backward overlap propagates the prior write",
            &[0xf3, 0xa4],
        )
        .stored_flags(record(0xff))
        .register(Ecx, 4, 0)
        .register(Esi, 0x4004, 0x4000)
        .register(Edi, 0x4003, 0x3fff)
        .memory(0x4000, &[1, 2, 3, 4, 5, 6], ReadWrite)
        .expect_memory(0x4000, &[5, 5, 5, 5, 5, 6]),
        Case::preserving_flags(
            "REP MOVSD captures each operand but sees preceding overlapping stores",
            &[0xf3, 0xa5],
        )
        .stored_flags(record(0xfe))
        .register(Ecx, 2, 0)
        .register(Esi, 0x4000, 0x4008)
        .register(Edi, 0x4001, 0x4009)
        .memory(0x4000, &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10], ReadWrite)
        .expect_memory(0x4000, &[1, 1, 2, 3, 4, 4, 6, 7, 8, 10]),
        Case::preserving_flags(
            "REP MOVSB physical aliases carry writes into the next read",
            &[0xf3, 0xa4],
        )
        .stored_flags(record(0xfe))
        .register(Ecx, 4, 0)
        .register(Esi, 0x4000, 0x4004)
        .register(Edi, 0x7001, 0x7005)
        .map_page(4, 0x8000, ReadOnly)
        .map_page(7, 0x8000, ReadWrite)
        .backing(0x8000, &[1, 2, 3, 4, 5, 6])
        .expect_memory(0x7001, &[1, 1, 1, 1]),
    ]
}

fn partial_fault_progress() -> Vec<Case> {
    let mut cases = Vec::new();
    for width in [1, 2, 4] {
        for operation in [Operation::Movs, Operation::Stos] {
            // Two writes complete; the third starts on an absent page.
            cases.push(Case::preserving_flags(
                format!("REP {operation:?} width {width} retains two writes before destination fault"),
                &rep(operation, width),
            )
            .stored_flags(record(0xfe)).register(Ecx, 4, 2)
            .register(Edi, 0x8000 - width * 2, 0x8000)
            .register(Esi, 0x4000, if operation == Operation::Movs { 0x4000 + width * 2 } else { 0x4000 })
            .initial_register(Eax, 0x1111_1111)
            .memory(0x4000, &[0x11; 16], ReadOnly)
            .memory(0x8000 - width * 2, &vec![0xa5; (width * 2) as usize], ReadWrite)
            .expect_memory(0x8000 - width * 2, &vec![0x11; (width * 2) as usize])
            .fault(0x8000, 2));
        }
        cases.push(Case::preserving_flags(
            format!("REP MOVS width {width} source fault precedes missing destination after progress"),
            &rep(Operation::Movs, width),
        )
        .stored_flags(record(0xfe)).register(Ecx, 3, 1)
        .register(Esi, 0x5000 - width * 2, 0x5000)
        .register(Edi, 0x8000 - width * 2, 0x8000)
        .memory(0x5000 - width * 2, &vec![0x11; (width * 2) as usize], ReadOnly)
        .memory(0x8000 - width * 2, &vec![0xa5; (width * 2) as usize], ReadWrite)
        .expect_memory(0x8000 - width * 2, &vec![0x11; (width * 2) as usize])
        .fault(0x5000, 0));
    }
    // A failed unaligned word write must not store even its first byte.
    cases.push(
        Case::preserving_flags(
            "REP MOVSW keeps one completed iteration and no partial second write",
            &[0xf3, 0x66, 0xa5],
        )
        .stored_flags(record(0xfe))
        .register(Ecx, 3, 2)
        .register(Esi, 0x4000, 0x4002)
        .register(Edi, 0x7ffd, 0x7fff)
        .memory(0x4000, &[0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc], ReadOnly)
        .memory(0x7ffd, &[0xa5, 0xa5, 0xa5], ReadWrite)
        .expect_memory(0x7ffd, &[0x12, 0x34, 0xa5])
        .fault(0x8000, 2),
    );
    cases.push(
        Case::preserving_flags(
            "REP MOVSW uses full ECX despite word operand override",
            &[0xf3, 0x66, 0xa5],
        )
        .stored_flags(record(0xfe))
        .register(Ecx, 0x10000, 0xffff)
        .register(Esi, 0x4ffe, 0x5000)
        .register(Edi, 0x7000, 0x7002)
        .memory(0x4ffe, &[0x12, 0x34], ReadOnly)
        .memory(0x7000, &[0xa5; 4], ReadWrite)
        .expect_memory(0x7000, &[0x12, 0x34])
        .fault(0x5000, 0),
    );
    cases.push(
        Case::preserving_flags(
            "REP MOVSB backward fault leaves current indices and remaining count",
            &[0xf3, 0xa4],
        )
        .stored_flags(record(0xff))
        .register(Ecx, 3, 1)
        .register(Esi, 0x4001, 0x3fff)
        .register(Edi, 0x7001, 0x6fff)
        .memory(0x4000, &[0x12, 0x34], ReadOnly)
        .memory(0x7000, &[0xa5; 2], ReadWrite)
        .expect_memory(0x7000, &[0x12, 0x34])
        .fault(0x3fff, 0),
    );
    cases
}

fn prefix_order_and_length() -> Vec<Case> {
    let mut cases = Vec::new();
    for code in [
        vec![0x66, 0xf3, 0xa5],
        vec![0xf3, 0x66, 0xa5],
        vec![0xf3, 0xf3, 0x66, 0xa5],
        [vec![0x66; 13], vec![0xf3, 0xa5]].concat(),
    ] {
        cases.push(
            Case::preserving_flags("REP MOVSW prefix order and fifteen-byte instruction", &code)
                .stored_flags(record(0xfe))
                .register(Ecx, 1, 0)
                .register(Esi, 0x4000, 0x4002)
                .register(Edi, 0x7000, 0x7002)
                .memory(0x4000, &[0x12, 0x34], ReadOnly)
                .memory(0x7000, &[0xa5; 2], ReadWrite)
                .expect_memory(0x7000, &[0x12, 0x34]),
        );
    }
    cases
}

test_cases!(rep_prefix_order_and_length, prefix_order_and_length());
test_cases!(zero_ecx_skips_operands_and_preserves_flags, zero_count());
test_cases!(
    one_and_three_iterations_width_df_and_accumulator,
    transfers()
);
test_cases!(
    final_element_updates_wrapping_indices_once,
    final_index_wrap()
);
test_cases!(sequential_overlapping_reads_and_writes, overlap());
test_cases!(
    faults_preserve_completed_iterations,
    partial_fault_progress()
);
