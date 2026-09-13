use super::{flags, record, Operation, OPERATIONS};
use crate::support::cases::{
    test_cases, InstructionCase as Case,
    Permissions::{ReadOnly, ReadWrite},
};
use wasm86_x86::Gpr32::{Eax, Ecx, Edi, Esi};

fn success(operation: Operation, width: u32, source: u32, destination: u32, df: u8) -> Case {
    let code = operation.code(width);
    let name = format!(
        "{operation:?} {width} bytes, ESI {source:08x}, EDI {destination:08x}, DF {df:02x}"
    );
    let case = if operation.compares() {
        Case::replacing_flags(name, &code, flags(10))
    } else {
        Case::preserving_flags(name, &code)
    };
    let advance = |address: u32| {
        if df & 1 == 0 {
            address.wrapping_add(width)
        } else {
            address.wrapping_sub(width)
        }
    };
    let mut case = case
        .stored_flags(record(df))
        .initial_registers(&[(Eax, 0x7856_3412), (Ecx, 7)]);
    if operation.uses_source_index() {
        case = case.register(Esi, source, advance(source));
    } else {
        case = case.initial_register(Esi, source);
    }
    if operation.uses_destination_index() {
        case = case.register(Edi, destination, advance(destination));
    } else {
        case = case.initial_register(Edi, destination);
    }
    if operation.writes_memory() {
        case = case.expect_memory(destination, &[0x12, 0x34, 0x56, 0x78][..width as usize]);
    }
    case
}

fn scattered_accesses() -> Vec<Case> {
    let mut cases = Vec::new();
    for width in [2, 4] {
        for first_bytes in 1..width {
            for operation in OPERATIONS {
                let source = 0x5000 - first_bytes;
                let destination = 0x8000 - first_bytes;
                let permissions = if operation.writes_memory() {
                    ReadWrite
                } else {
                    ReadOnly
                };
                let mut case = success(operation, width, source, destination, 0xff);
                if operation.uses_source_index() {
                    case = case
                        .map_page(4, 0x8000, ReadOnly)
                        .map_page(5, 0xa000, ReadOnly)
                        .memory(
                            source,
                            &[0x12, 0x34, 0x56, 0x78][..width as usize],
                            ReadOnly,
                        );
                }
                if operation.uses_destination_index() {
                    case = case
                        .map_page(7, 0xc000, permissions)
                        .map_page(8, 0xe000, permissions)
                        .memory(
                            destination,
                            &if operation.writes_memory() {
                                [0xa5; 4]
                            } else {
                                [0x12, 0x34, 0x56, 0x78]
                            }[..width as usize],
                            permissions,
                        );
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn guest_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in OPERATIONS {
        for width in [1, 2, 4] {
            // Missing source takes precedence over any MOVS destination write or CMPS read.
            cases.push(
                Case::preserving_flags(
                    format!("{operation:?} {width} bytes: first access absent"),
                    &operation.code(width),
                )
                .stored_flags(record(0x81))
                .initial_registers(&[(Esi, 0x4000), (Edi, 0x7000), (Eax, 0x4433_2211)])
                .fault(
                    if operation.uses_source_index() {
                        0x4000
                    } else {
                        0x7000
                    },
                    if operation == Operation::Stos { 2 } else { 0 },
                ),
            );
            if operation.uses_destination_index() {
                let mut case = Case::preserving_flags(
                    format!("{operation:?} {width} bytes: destination absent after valid source"),
                    &operation.code(width),
                )
                .stored_flags(record(0xfe))
                .initial_registers(&[(Esi, 0x4000), (Edi, 0x7000), (Eax, 0x4433_2211)])
                .fault(0x7000, if operation.writes_memory() { 2 } else { 0 });
                if operation.uses_source_index() {
                    case = case.memory(0x4000, &[0x12, 0x34, 0x56, 0x78], ReadOnly);
                }
                cases.push(case);
            }
            if operation.writes_memory() {
                cases.push(
                    Case::preserving_flags(
                        format!("{operation:?} {width} bytes: read-only destination"),
                        &operation.code(width),
                    )
                    .stored_flags(record(0xff))
                    .initial_registers(&[(Esi, 0x4000), (Edi, 0x7000), (Eax, 0x4433_2211)])
                    .memory(0x4000, &[0x12, 0x34, 0x56, 0x78], ReadOnly)
                    .memory(0x7000, &[0xa5; 4], ReadOnly)
                    .fault(0x7000, 3),
                );
            }
        }
        for width in [2, 4] {
            if operation.uses_source_index() {
                cases.push(
                    Case::preserving_flags(
                        format!("{operation:?} {width} bytes: source second page absent"),
                        &operation.code(width),
                    )
                    .stored_flags(record(0xff))
                    .initial_registers(&[(Esi, 0x4fff), (Edi, 0x7000)])
                    .map_page(4, 0x8000, ReadOnly)
                    .backing(0x8fff, &[0x12])
                    .memory(0x7000, &[0xa5; 4], ReadWrite)
                    .fault(0x5000, 0),
                );
            }
            if operation.uses_destination_index() {
                for protected in [false, true] {
                    if protected && !operation.writes_memory() {
                        continue;
                    }
                    let name = format!("{operation:?} {width} bytes: destination second page, protected {protected}");
                    let error = if protected {
                        3
                    } else if operation.writes_memory() {
                        2
                    } else {
                        0
                    };
                    let mut case = Case::preserving_flags(name, &operation.code(width))
                        .stored_flags(record(0xfe))
                        .initial_registers(&[(Esi, 0x4000), (Edi, 0x7fff), (Eax, 0x4433_2211)])
                        .memory(0x4000, &[0x12, 0x34, 0x56, 0x78], ReadOnly)
                        .map_page(7, 0xc000, ReadWrite)
                        .backing(0xcffe, &[0x5a, 0xa5])
                        .backing(0xe000, &[0xa5; 4])
                        .fault(0x8000, error);
                    if protected {
                        case = case.map_page(8, 0xe000, ReadOnly);
                    }
                    cases.push(case);
                }
            }
        }
    }
    cases
}

fn index_wrap_and_page_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in OPERATIONS {
        for width in [1, 2, 4] {
            for (address, df) in [
                (0u32.wrapping_sub(width), 0xfe),
                (0, 0xff),
                (0x1234_fffe, 0x80),
            ] {
                let mut case = success(operation, width, address, address, df);
                case = case.memory(
                    address,
                    &[0x12, 0x34, 0x56, 0x78][..width as usize],
                    if operation.writes_memory() {
                        ReadWrite
                    } else {
                        ReadOnly
                    },
                );
                cases.push(case);
            }
        }
        for width in [2, 4] {
            let address = u32::MAX;
            cases.push(
                Case::preserving_flags(
                    format!(
                        "{operation:?} {width} byte wrapped operand reaches an absent page zero"
                    ),
                    &operation.code(width),
                )
                .stored_flags(record(0x81))
                .initial_registers(&[(Esi, address), (Edi, address)])
                .map_page(0xfffff, 0x8000, ReadWrite)
                .backing(0x8fff, &[0x12])
                .backing(0xa000, &[0x34, 0x56, 0x78])
                .fault(0, if operation == Operation::Stos { 2 } else { 0 }),
            );
        }
    }
    cases
}

fn overlapping_and_aliased_operands() -> Vec<Case> {
    let mut cases = Vec::new();
    for (source, destination, output) in [
        (0x4000, 0x4001, vec![1, 1, 2, 3, 4, 6]),
        (0x4001, 0x4000, vec![2, 3, 4, 5, 5, 6]),
    ] {
        cases.push(
            Case::preserving_flags(
                "MOVSD loads the entire overlapping source before writing",
                &[0xa5],
            )
            .stored_flags(record(0xfe))
            .register(Esi, source, source + 4)
            .register(Edi, destination, destination + 4)
            .memory(0x4000, &[1, 2, 3, 4, 5, 6], ReadWrite)
            .expect_memory(0x4000, &output),
        );
    }
    cases.push(
        Case::preserving_flags(
            "MOVSD captures an overlapping source across scattered pages",
            &[0xa5],
        )
        .stored_flags(record(0xfe))
        .register(Esi, 0x4ffe, 0x5002)
        .register(Edi, 0x4fff, 0x5003)
        .map_page(4, 0x8000, ReadWrite)
        .map_page(5, 0xa000, ReadWrite)
        .memory(0x4ffd, &[1, 2, 3, 4, 5, 6, 7], ReadWrite)
        .expect_memory(0x4ffd, &[1, 2, 2, 3, 4, 5, 7]),
    );
    cases.push(
        Case::preserving_flags(
            "MOVSD physical alias uses source bytes before the overlapping write",
            &[0xa5],
        )
        .stored_flags(record(0xff))
        .register(Esi, 0x4000, 0x3ffc)
        .register(Edi, 0x7001, 0x6ffd)
        .map_page(4, 0x8000, ReadOnly)
        .map_page(7, 0x8000, ReadWrite)
        .backing(0x8000, &[1, 2, 3, 4, 5, 6])
        .expect_memory(0x7001, &[1, 2, 3, 4]),
    );
    cases.push(
        Case::replacing_flags(
            "CMPSD permits read-only physical aliases and compares equal",
            &[0xa7],
            flags(10),
        )
        .stored_flags(record(0xfe))
        .register(Esi, 0x4000, 0x4004)
        .register(Edi, 0x7000, 0x7004)
        .map_page(4, 0x8000, ReadOnly)
        .map_page(7, 0x8000, ReadOnly)
        .backing(0x8000, &[1, 2, 3, 4]),
    );
    cases
}

test_cases!(scattered_pages, scattered_accesses());
test_cases!(atomic_guest_faults_and_access_order, guest_faults());
test_cases!(
    full_width_indices_and_wrapped_page_faults,
    index_wrap_and_page_faults()
);
test_cases!(
    overlap_and_physical_aliases,
    overlapping_and_aliased_operands()
);
