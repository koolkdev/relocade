use super::{flags, record, Operation, OPERATIONS};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::{
    Gpr32::{Eax, Ecx, Edi, Esi},
    StoredFlags, StoredStatusSource,
};

fn transfers() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in [Operation::Movs, Operation::Stos, Operation::Lods] {
        for width in [1, 2, 4] {
            for df in [0x80, 0xfe, 0x81, 0xff] {
                let advance = |address: u32| {
                    if df & 1 == 0 {
                        address + width
                    } else {
                        address - width
                    }
                };
                let mut case = Case::preserving_flags(
                    format!("{operation:?} {width} bytes, raw DF {df:02x}"),
                    &operation.code(width),
                )
                .stored_flags(record(df))
                .initial_register(Ecx, 0);
                if operation.uses_source_index() {
                    case = case.register(Esi, 0x4001, advance(0x4001)).memory(
                        0x4000,
                        &[0x5a, 0x12, 0x34, 0x56, 0x78, 0xa5],
                        ReadOnly,
                    );
                } else {
                    case = case.initial_register(Esi, 0x4001);
                }
                if operation.uses_destination_index() {
                    let data = if operation == Operation::Movs {
                        [0x12, 0x34, 0x56, 0x78]
                    } else {
                        [0xd4, 0xc3, 0xb2, 0xa1]
                    };
                    case = case
                        .initial_register(Eax, 0xa1b2_c3d4)
                        .register(Edi, 0x6001, advance(0x6001))
                        .memory(0x6000, &[0xa5; 6], ReadWrite)
                        .expect_memory(0x6001, &data[..width as usize]);
                } else {
                    let output = match width {
                        1 => 0xa1b2_c312,
                        2 => 0xa1b2_3412,
                        4 => 0x7856_3412,
                        _ => unreachable!(),
                    };
                    case = case
                        .initial_register(Edi, 0x6001)
                        .register(Eax, 0xa1b2_c3d4, output);
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn comparisons() -> Vec<Case> {
    let mut cases = Vec::new();
    // CF/PF/AF/ZF/SF/OF masks are literal SUB results. Low-byte parity is used at every width.
    for (width, sign, maximum) in [
        (1, 0x80u32, 0xffu32),
        (2, 0x8000, 0xffff),
        (4, 0x8000_0000, u32::MAX),
    ] {
        for (name, left, right, status) in [
            ("equal", 0x55, 0x55, 10),
            ("borrow and negative", 0, 1, 23),
            (
                "signed minimum minus one",
                sign,
                1,
                if width == 1 { 36 } else { 38 },
            ),
            (
                "positive overflow",
                sign - 1,
                maximum,
                if width == 1 { 49 } else { 51 },
            ),
            ("even low-byte parity", 3, 0, 2),
            ("auxiliary borrow", 0x10, 1, 6),
            ("positive odd parity", 2, 1, 0),
            ("negative without overflow", maximum, 0, 18),
        ] {
            for operation in [Operation::Cmps, Operation::Scas] {
                let df = if status & 1 == 0 { 0xfe } else { 0x81 };
                let advance = |address: u32| {
                    if df & 1 == 0 {
                        address + width
                    } else {
                        address - width
                    }
                };
                let mut case = Case::replacing_flags(
                    format!("{operation:?} {width} bytes: {name}"),
                    &operation.code(width),
                    flags(status),
                )
                .stored_flags(record(df))
                .initial_register(Ecx, 0xffff_ffff)
                .register(Edi, 0x6001, advance(0x6001))
                .memory(0x6001, &right.to_le_bytes()[..width as usize], ReadOnly);
                if operation == Operation::Cmps {
                    case = case
                        .register(Esi, 0x4001, advance(0x4001))
                        .memory(0x4001, &left.to_le_bytes()[..width as usize], ReadOnly)
                        .initial_register(Eax, 0xfedc_ba98);
                } else {
                    // Narrow SCAS must ignore the disagreeing upper accumulator bits.
                    let accumulator = match width {
                        1 => 0xabcd_ef00 | left,
                        2 => 0xabcd_0000 | left,
                        _ => left,
                    };
                    case = case
                        .initial_register(Esi, 0x4001)
                        .initial_register(Eax, accumulator);
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn stored_recipes() -> Vec<Case> {
    let mut cases = Vec::new();
    for (index, (kind, left, right)) in [
        (1, 0xabcd_0000, 0x1234_0001),
        (2, 127, 1),
        (3, 0x80, 0),
        (5, 0x8000, 1),
        (6, 0xffff, 1),
        (7, 0x8000, 0),
        (9, 0x8000_0000, 1),
        (10, u32::MAX, 1),
        (11, 0, 0),
    ]
    .into_iter()
    .enumerate()
    {
        let width = [1, 2, 4][index % 3];
        let operation = [Operation::Movs, Operation::Stos, Operation::Lods][index % 3];
        let stored = StoredFlags {
            status_source: StoredStatusSource {
                kind,
                left,
                right,
                ..record(0xff).status_source
            },
            ..record(0xff)
        };
        let mut case = Case::preserving_flags(
            format!("{operation:?} preserves stored status recipe {kind}"),
            &operation.code(width),
        )
        .stored_flags(stored)
        .initial_register(Eax, 0x7856_3412);
        if operation.uses_source_index() {
            case = case.register(Esi, 0x4000, 0x4000 - width).memory(
                0x4000,
                &[0x12, 0x34, 0x56, 0x78],
                ReadOnly,
            );
        } else {
            case = case.initial_register(Esi, 0x4000);
        }
        if operation.uses_destination_index() {
            case = case
                .register(Edi, 0x6000, 0x6000 - width)
                .memory(0x6000, &[0xa5; 4], ReadWrite)
                .expect_memory(0x6000, &[0x12, 0x34, 0x56, 0x78][..width as usize]);
        } else {
            case = case.initial_register(Edi, 0x6000);
        }
        cases.push(case);
    }
    for operation in OPERATIONS
        .into_iter()
        .filter(|operation| operation.compares())
    {
        cases.push(
            Case::replacing_flags(
                format!("{operation:?} replaces a stored ADD recipe"),
                &operation.code(2),
                flags(23),
            )
            .stored_flags(StoredFlags {
                status_source: StoredStatusSource {
                    kind: 10,
                    left: u32::MAX,
                    right: 1,
                    ..record(0xfe).status_source
                },
                ..record(0xfe)
            })
            .initial_register(Eax, 0x1234_0000)
            .register(Edi, 0x6000, 0x6002)
            .memory(0x6000, &[1, 0], ReadOnly)
            .register(
                Esi,
                0x4000,
                if operation.uses_source_index() {
                    0x4002
                } else {
                    0x4000
                },
            )
            .memory(0x4000, &[0, 0], ReadOnly),
        );
    }
    cases
}

test_cases!(transfer_widths_and_raw_direction, transfers());
test_cases!(literal_comparison_flags, comparisons());
test_cases!(stored_status_is_preserved_or_replaced, stored_recipes());
