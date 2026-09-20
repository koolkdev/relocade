use super::*;

fn accesses() -> Vec<Case> {
    let mut cases = Vec::new();
    for word in [false, true] {
        let width = if word { 2 } else { 4 };
        let bytes = pair(word, -5, 7);
        for default16 in [false, true] {
            let case = |name: &str, start, index: i32| {
                let case = Case::preserving_flags(
                    format!("BOUND {name} word={word} CS.D16={default16}"),
                    &absolute_code(word, default16, 0, start),
                )
                .initial_register(Eax, index as u32);
                if default16 {
                    case.segmented_only().segment(Segment::Cs, code16())
                } else {
                    case
                }
            };
            for split in 1..2 * width {
                let start = 0x5000 - split as u32;
                cases.push(
                    case(&format!("scattered pair split at {split}"), start, 7)
                        .map_page(4, 0x8000, ReadOnly)
                        .map_page(5, 0xa000, ReadOnly)
                        .memory(start, &bytes, ReadOnly),
                );
            }
            cases.push(
                case("exact readable page end", 0x5000 - 2 * width as u32, 7).memory(
                    0x5000 - 2 * width as u32,
                    &bytes,
                    ReadOnly,
                ),
            );
            cases.push(case("missing lower bound", 0x4000, -6).fault(0x4000, 0));
            cases.push(
                case(
                    "lower field straddles denied page before comparison",
                    0x4fff,
                    -6,
                )
                .memory(0x4fff, &bytes[..1], ReadOnly)
                .fault(0x5000, 0),
            );
            for index in [-6, -5, 8] {
                let start = 0x5000 - width as u32;
                let mut upper = case(
                    &format!("inaccessible upper with index {index}"),
                    start,
                    index,
                )
                .memory(start, &bytes[..width], ReadOnly);
                upper = if index == -6 {
                    upper.bound_range_exceeded()
                } else {
                    upper.fault(0x5000, 0)
                };
                cases.push(upper);
            }
            cases.push(
                case(
                    "upper field straddles denied page",
                    0x5000 - width as u32 - 1,
                    8,
                )
                .memory(0x5000 - width as u32 - 1, &bytes[..width + 1], ReadOnly)
                .fault(0x5000, 0),
            );
        }
    }
    cases
}

fn address_selection() -> Vec<Case> {
    vec![
        Case::preserving_flags("index register also supplies the base", &[0x62, 0x1b])
            .initial_register(Ebx, 0x4000)
            .memory(0x4000, &pair(false, 0x3fff, 0x4000), ReadOnly),
        Case::preserving_flags(
            "SP index and SIB base use SS independently of SS.B",
            &[0x66, 0x62, 0x64, 0x24, 0x80],
        )
        .segmented_only()
        .initial_register(Esp, 0x1_4080)
        .segment(Segment::Ss, segment(0x20000, 0x2ffff, 0x05))
        .segment(Segment::Ds, StoredSegment::unusable(0))
        .memory(0x34000, &pair(true, 0x4000, 0x4080), ReadOnly),
        Case::preserving_flags(
            "BP in address16 selects SS and wraps initial sum",
            &[0x67, 0x62, 0x02],
        )
        .segmented_only()
        .initial_registers(&[(Eax, 7), (Ebp, 0xabcd_f000), (Esi, 0x9876_5000)])
        .segment(Segment::Ss, segment(0x20000, u32::MAX, 0x15))
        .segment(Segment::Ds, StoredSegment::unusable(0))
        .memory(0x24000, &pair(false, -5, 7), ReadOnly),
        Case::preserving_flags(
            "SIB with EBP only as index uses DS",
            &[0x62, 0x04, 0xad, 0, 0x40, 0, 0],
        )
        .segmented_only()
        .initial_registers(&[(Eax, -1i32 as u32), (Ebp, 4)])
        .segment(Segment::Ss, StoredSegment::unusable(0))
        .segment(Segment::Ds, segment(0x20000, u32::MAX, 0x15))
        .memory(0x24010, &pair(false, -5, 7), ReadOnly),
        Case::preserving_flags(
            "last segment override applies to both bounds",
            &[0x65, 0x36, 0x64, 0x62, 0x03],
        )
        .initial_registers(&[(Eax, 7), (Ebx, 0x4000)])
        .segment(Segment::Fs, segment(0x20000, 0x4007, 0x01))
        .segment(Segment::Gs, StoredSegment::unusable(0))
        .memory(0x24000, &pair(false, -5, 7), ReadOnly),
        Case::preserving_flags(
            "CS16 with address32 and operand32 overrides",
            &[0x67, 0x66, 0x62, 0x03],
        )
        .segmented_only()
        .segment(Segment::Cs, code16())
        .initial_registers(&[(Eax, 7), (Ebx, 0x14000)])
        .memory(0x14000, &pair(false, -5, 7), ReadOnly),
    ]
}

fn boundaries() -> Vec<Case> {
    let mut cases = Vec::new();
    for word in [false, true] {
        let width = if word { 2 } else { 4 };
        let bytes = pair(word, -5, 7);
        let prefix = if word { &[0x66][..] } else { &[][..] };
        for (segment_register, override_byte, fault_ss) in
            [(Segment::Ds, 0x3e, false), (Segment::Ss, 0x36, true)]
        {
            let code = [prefix, &[override_byte, 0x62, 0x03]].concat();
            let base = |limit, index: i32| {
                Case::preserving_flags(
                    format!("BOUND {segment_register:?} limit={limit:x} index={index} word={word}"),
                    &code,
                )
                .segmented_only()
                .initial_registers(&[(Eax, index as u32), (Ebx, 0x4000)])
                .segment(segment_register, segment(0x20000, limit, 0x15))
            };
            let segment_fault = |case: Case| {
                if fault_ss {
                    case.stack_fault(0)
                } else {
                    case.general_protection(0)
                }
            };
            cases.push(base(0x4000 + 2 * width - 1, 7).memory(0x24000, &bytes, ReadOnly));
            cases.push(segment_fault(
                base(0x4000 + 2 * width - 2, 8).memory(0x24000, &bytes, ReadOnly),
            ));
            cases.push(segment_fault(base(0x4000 + width - 2, -6)));
            // A lower-bound failure wins over an inaccessible upper segment field.
            cases.push(
                base(0x4000 + width - 1, -6)
                    .memory(0x24000, &bytes[..width as usize], ReadOnly)
                    .bound_range_exceeded(),
            );
            cases.push(segment_fault(
                Case::preserving_flags(
                    format!("BOUND unusable {segment_register:?} word={word}"),
                    &code,
                )
                .segmented_only()
                .initial_registers(&[(Eax, -6i32 as u32), (Ebx, 0x4000)])
                .segment(segment_register, StoredSegment::unusable(0)),
            ));
            cases.push(
                Case::preserving_flags(
                    format!("BOUND expand-down {segment_register:?} word={word}"),
                    &code,
                )
                .segmented_only()
                .initial_registers(&[(Eax, 7), (Ebx, 0x4000)])
                .segment(segment_register, segment(0, 0x3fff, 0x0d))
                .memory(0x4000, &bytes, ReadOnly),
            );
            cases.push(segment_fault(
                Case::preserving_flags(
                    format!(
                        "BOUND upper exceeds small expand-down {segment_register:?} word={word}"
                    ),
                    &code,
                )
                .segmented_only()
                .initial_registers(&[(Eax, 7), (Ebx, 0x10000 - width)])
                .segment(segment_register, segment(0, 0x3fff, 0x0d))
                .memory(0x10000 - width, &bytes[..width as usize], ReadOnly),
            ));
        }
        for default16 in [false, true] {
            // Address size wraps each field's start, independently of its data width.
            let mut code = Vec::new();
            if word != default16 {
                code.push(0x66);
            }
            if !default16 {
                code.push(0x67);
            }
            code.extend([0x62, 0x06]);
            code.extend_from_slice(&((0x10000 - width) as u16).to_le_bytes());
            for present in [false, true] {
                let mut case = Case::preserving_flags(
                    format!("BOUND wrapped upper word={word} CS.D16={default16} present={present}"),
                    &code,
                )
                .initial_register(Eax, 7)
                .memory(0x10000 - width, &bytes[..width as usize], ReadOnly)
                .memory(0x10000, &pair(word, -1, -1), ReadOnly);
                if default16 {
                    case = case.segmented_only().segment(Segment::Cs, code16());
                }
                cases.push(if present {
                    case.memory(0, &bytes[width as usize..], ReadOnly)
                } else {
                    case.fault(0, 0)
                });
            }
        }
        // A straddling lower value does not wrap its individual bytes.
        let code = [prefix, &[0x67, 0x62, 0x07]].concat(); // [BX]
        cases.push(
            Case::preserving_flags(format!("BOUND lower field spans 64K word={word}"), &code)
                .initial_registers(&[(Eax, 7), (Ebx, 0xffff)])
                .memory(0xffff, &bytes[..width as usize], ReadOnly)
                .memory(width - 1, &bytes[width as usize..], ReadOnly),
        );
        cases.push(
            Case::preserving_flags(
                format!("BOUND lower field exceeds 64K segment word={word}"),
                &code,
            )
            .segmented_only()
            .initial_registers(&[(Eax, -6i32 as u32), (Ebx, 0xffff)])
            .segment(Segment::Ds, segment(0, 0xffff, 0x05))
            .general_protection(0),
        );
        let start = 0u32.wrapping_sub(width);
        cases.push(
            Case::preserving_flags(
                format!("BOUND upper address wraps at 32 bits word={word}"),
                &absolute_code(word, false, 0, start),
            )
            .initial_register(Eax, 7)
            .map_page(0xfffff, 0x8000, ReadOnly)
            .map_page(0, 0xa000, ReadOnly)
            .memory(start, &bytes[..width as usize], ReadOnly)
            .memory(0, &bytes[width as usize..], ReadOnly),
        );
    }
    cases
}

test_cases!(ordered_memory_accesses, accesses());
test_cases!(address_and_segment_selection, address_selection());
test_cases!(field_wrapping_and_segment_limits, boundaries());
