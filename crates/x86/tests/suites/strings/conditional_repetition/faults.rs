use super::*;
use crate::support::cases::{test_cases, Permissions::ReadOnly};
use wasm86_x86::Gpr32::{Eax, Ecx, Edi, Esi};

fn page_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            for width in [1, 2, 4] {
                for completed in [0, 2] {
                    for source_fault in [false, true] {
                        if source_fault && operation == Operation::Scas {
                            continue;
                        }
                        let source = if source_fault {
                            0x5000 - completed * width
                        } else {
                            0x4000
                        };
                        let destination = if source_fault {
                            0x7000
                        } else {
                            0x8000 - completed * width
                        };
                        let right = if prefix == 0xf3 { 5 } else { 3 };
                        let mut stored = record(0xfe);
                        stored.status_source.kind = 9;
                        let mut case = Case::preserving_flags(
                            format!("{operation:?} {prefix:02x} width {width}, {completed} complete before source_fault {source_fault}"),
                            &code(operation, prefix, width, false, false),
                        ).stored_flags(stored).instruction_count(17)
                            .register(Ecx, 3, 3 - completed)
                            .register(Edi, destination, destination + completed * width)
                            .initial_register(Eax, 5)
                            .fault(if source_fault { 0x5000 } else { 0x8000 }, 0);
                        if operation == Operation::Cmps {
                            case = case.register(Esi, source, source + completed * width);
                            let accessible = if source_fault { completed } else { 3 };
                            if accessible != 0 {
                                case = case.memory(
                                    source,
                                    &bytes(&vec![5; accessible as usize], width, false),
                                    ReadOnly,
                                );
                            }
                        }
                        let accessible = if source_fault { 3 } else { completed };
                        if accessible != 0 {
                            case = case.memory(
                                destination,
                                &bytes(&vec![right; accessible as usize], width, false),
                                ReadOnly,
                            );
                        }
                        cases.push(case);
                    }
                }
            }
        }
    }
    cases.push(
        Case::preserving_flags(
            "CMPS reads its source before the missing ES operand",
            &[0xf3, 0xa6],
        )
        .stored_flags(record(0xfe))
        .initial_register(Ecx, 2)
        .initial_register(Esi, 0x5000)
        .initial_register(Edi, 0x8000)
        .fault(0x5000, 0),
    );
    cases
}

fn split_elements() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            for width in [2, 4] {
                let right = if prefix == 0xf3 { 5 } else { 3 };
                // One element is complete. The next spans an absent page.
                let start = 0x8000 - width - 1;
                let mut data = bytes(&[right], width, false);
                data.push(right as u8);
                let mut case = Case::preserving_flags(
                    format!("split {operation:?} {prefix:02x} width {width} does not consume a partial element"),
                    &code(operation, prefix, width, false, false),
                ).stored_flags(record(0xfe)).register(Ecx, 3, 2)
                    .register(Edi, start, start + width).initial_register(Eax, 5)
                    .memory(start, &data, ReadOnly).fault(0x8000, 0);
                if operation == Operation::Cmps {
                    case = case.register(Esi, 0x4000, 0x4000 + width).memory(
                        0x4000,
                        &bytes(&[5; 3], width, false),
                        ReadOnly,
                    );
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn early_stop() -> Vec<Case> {
    let mut cases = Vec::new();
    for operation in COMPARISONS {
        for prefix in [0xf2, 0xf3] {
            for width in [1, 2, 4] {
                let right = if prefix == 0xf3 { 3 } else { 5 };
                let mut case = Case::replacing_flags(
                    format!("{operation:?} {prefix:02x} width {width} stops before the absent next page"),
                    &code(operation, prefix, width, false, false), flags(if prefix == 0xf3 { 0 } else { 10 }),
                ).stored_flags(record(0xfe)).register(Ecx, 3, 2)
                    .register(Edi, 0x8000 - width, 0x8000).initial_register(Eax, 5)
                    .memory(0x8000 - width, &bytes(&[right], width, false), ReadOnly);
                if operation == Operation::Cmps {
                    case = case.register(Esi, 0x5000 - width, 0x5000).memory(
                        0x5000 - width,
                        &bytes(&[5], width, false),
                        ReadOnly,
                    );
                }
                cases.push(case);
            }
        }
    }
    cases
}

fn segments_and_aliases() -> Vec<Case> {
    let mut cases = Vec::new();
    for prefix in [0xf2, 0xf3] {
        for default16 in [false, true] {
            cases.push(profile(
                Case::preserving_flags(
                    "word comparison uses full ECX with 32-bit addressing",
                    &code(Operation::Scas, prefix, 2, false, default16),
                )
                .stored_flags(record(0xfe))
                .initial_register(Eax, 5)
                .register(Ecx, 0x10000, 0xffff)
                .register(Edi, 0x17ffe, 0x18000)
                .memory(0x17ffe, &[if prefix == 0xf3 { 5 } else { 3 }, 0], ReadOnly)
                .fault(0x18000, 0),
                default16,
            ));
        }
        for default16 in [false, true] {
            for operation in COMPARISONS {
                let right = if prefix == 0xf3 { 5 } else { 3 };
                let mut case = Case::preserving_flags(
                    format!("{operation:?} {prefix:02x} segment fault retains 16-bit progress, CS.D16 {default16}"),
                    &code(operation, prefix, 1, true, default16),
                ).stored_flags(record(0xfe)).segmented_only()
                    .segment(Segment::Es, StoredSegment { base: 0x7000, limit: 0, ..StoredSegment::flat_data32(0x23) })
                    .register(Ecx, 0xaaaa_0003, 0xaaaa_0002)
                    .register(Edi, 0xbbbb_0000, 0xbbbb_0001)
                    .initial_register(Eax, 5).memory(0x7000, &[right], ReadOnly).general_protection(0);
                if operation == Operation::Cmps {
                    case = case
                        .register(Esi, 0xcccc_4000, 0xcccc_4001)
                        .memory(0x4000, &[5; 3], ReadOnly);
                }
                cases.push(profile(case, default16));
            }
        }
        cases.push(
            Case::preserving_flags(
                "SS override faults before ES and restores entry flags",
                &[prefix, 0x36, 0xa6],
            )
            .stored_flags(record(0xfe))
            .segmented_only()
            .segment(
                Segment::Ss,
                StoredSegment {
                    base: 0x4000,
                    limit: 0,
                    ..StoredSegment::flat_data32(0x23)
                },
            )
            .register(Ecx, 3, 2)
            .register(Esi, 0, 1)
            .register(Edi, 0x7000, 0x7001)
            .memory(0x4000, &[5], ReadOnly)
            .memory(0x7000, &[if prefix == 0xf3 { 5 } else { 3 }; 3], ReadOnly)
            .stack_fault(0),
        );
        cases.push(
            Case::replacing_flags(
                "CMPS uses the last source override and fixed ES",
                &[prefix, 0x65, 0x64, 0xa6],
                flags(if prefix == 0xf3 { 0 } else { 10 }),
            )
            .stored_flags(record(0xfe))
            .segmented_only()
            .segment(Segment::Ds, StoredSegment::unusable(0))
            .segment(Segment::Gs, StoredSegment::unusable(0))
            .segment(
                Segment::Fs,
                StoredSegment {
                    base: 0x4000,
                    ..StoredSegment::flat_data32(0x23)
                },
            )
            .segment(
                Segment::Es,
                StoredSegment {
                    base: 0x7000,
                    ..StoredSegment::flat_data32(0x23)
                },
            )
            .register(Ecx, 3, 2)
            .register(Esi, 0, 1)
            .register(Edi, 0, 1)
            .memory(0x4000, &[5], ReadOnly)
            .memory(0x7000, &[if prefix == 0xf3 { 3 } else { 5 }], ReadOnly),
        );
        cases.push(
            Case::replacing_flags(
                "SCAS ignores an unusable source override",
                &[prefix, 0x64, 0xae],
                flags(if prefix == 0xf3 { 0 } else { 10 }),
            )
            .stored_flags(record(0xfe))
            .segment(Segment::Fs, StoredSegment::unusable(0))
            .initial_register(Eax, 5)
            .initial_register(Esi, 0x9000)
            .register(Ecx, 3, 2)
            .register(Edi, 0x7000, 0x7001)
            .memory(0x7000, &[if prefix == 0xf3 { 3 } else { 5 }], ReadOnly),
        );
    }
    cases
}

test_cases!(
    page_faults_keep_entry_flags_and_completed_progress,
    page_faults()
);
test_cases!(
    split_elements_do_not_change_flags_or_indices,
    split_elements()
);
test_cases!(early_termination_skips_later_faults, early_stop());
test_cases!(
    segment_selection_and_address_aliases_survive_faults,
    segments_and_aliases()
);
