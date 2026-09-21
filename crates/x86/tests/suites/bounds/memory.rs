//! BOUND reads and compares the lower field before accessing the upper field.
use super::{absolute_code, code16, pair, segment};
use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};
use wasm86_x86::{Gpr32::*, Segment};

fn ordered_accesses() -> Vec<Case> {
    let mut cases = Vec::new();
    for word in [false, true] {
        let width = if word { 2 } else { 4 };
        let bytes = pair(word, -5, 7);
        let case = |name: &str, address, index: i32| {
            Case::preserving_flags(
                format!("BOUND {name}, word={word}"),
                &absolute_code(word, false, 0, address),
            )
            .initial_register(Eax, index as u32)
        };
        // The page break lies inside the lower field, between fields, or inside the upper field.
        for split in [1, width, width + 1] {
            let address = 0x5000 - split as u32;
            cases.push(
                case("scattered pair", address, 7)
                    .map_page(4, 0x8000, ReadOnly)
                    .map_page(5, 0xa000, ReadOnly)
                    .memory(address, &bytes, ReadOnly),
            );
        }
        let end = 0x5000 - 2 * width as u32;
        cases.push(
            case("pair ends at the last readable byte", end, 7).memory(end, &bytes, ReadOnly),
        );
        cases.push(case("missing lower field", 0x4000, -6).fault(0x4000, 0));
        cases.push(
            case("lower field must be read completely", 0x4fff, -6)
                .memory(0x4fff, &bytes[..1], ReadOnly)
                .fault(0x5000, 0),
        );
        for index in [-6, -5, 8] {
            let address = 0x5000 - width as u32;
            let case = case("inaccessible upper field", address, index).memory(
                address,
                &bytes[..width],
                ReadOnly,
            );
            cases.push(if index == -6 {
                case.bound_range_exceeded()
            } else {
                case.fault(0x5000, 0)
            });
        }
        let address = 0x5000 - width as u32 - 1;
        cases.push(
            case("upper field must be read completely", address, 8)
                .memory(address, &bytes[..width + 1], ReadOnly)
                .fault(0x5000, 0),
        );
    }
    cases
}

test_cases!(
    conditional_upper_reads_and_complete_fields,
    ordered_accesses()
);

#[rustfmt::skip]
fn address_dependencies() -> Vec<Case> {
    vec![
        Case::preserving_flags("index register also supplies the memory base", &[0x62, 0x1b])
            .initial_register(Ebx, 0x4000).memory(0x4000, &pair(false, 0x3fff, 0x4000), ReadOnly),
        Case::preserving_flags("the segment override applies to both bounds", &[0x64, 0x62, 0x03])
            .initial_registers(&[(Eax, 7), (Ebx, 0x4000)])
            .segment(Segment::Fs, segment(0x20000, 0x4007, 0x01))
            .memory(0x24000, &pair(false, -5, 7), ReadOnly),
        Case::preserving_flags("CS16 combines address32 and operand32 overrides", &[0x67, 0x66, 0x62, 0x03])
            .segmented_only().segment(Segment::Cs, code16())
            .initial_registers(&[(Eax, 7), (Ebx, 0x14000)]).memory(0x14000, &pair(false, -5, 7), ReadOnly),
    ]
}
test_cases!(index_alias_and_segmented_fields, address_dependencies());

fn segment_ordering() -> Vec<Case> {
    let mut cases = Vec::new();
    for (word, register, prefix) in [
        (false, Segment::Ds, &[0x3e][..]),
        (true, Segment::Ss, &[0x66, 0x36][..]),
    ] {
        let width = if word { 2 } else { 4 };
        let bytes = pair(word, -5, 7);
        let code = [prefix, &[0x62, 0x03]].concat();
        let case = |limit, index: i32| {
            Case::preserving_flags(
                format!("BOUND {register:?}, limit={limit:x}, index={index}"),
                &code,
            )
            .segmented_only()
            .initial_registers(&[(Eax, index as u32), (Ebx, 0x4000)])
            .segment(register, segment(0x20000, limit, 0x15))
        };
        let fault = |case: Case| {
            if register == Segment::Ss {
                case.stack_fault(0)
            } else {
                case.general_protection(0)
            }
        };
        cases.push(case(0x4000 + 2 * width - 1, 7).memory(0x24000, &bytes, ReadOnly));
        cases.push(fault(
            case(0x4000 + 2 * width - 2, 8).memory(0x24000, &bytes, ReadOnly),
        ));
        cases.push(fault(case(0x4000 + width - 2, -6)));
        cases.push(
            case(0x4000 + width - 1, -6)
                .memory(0x24000, &bytes[..width as usize], ReadOnly)
                .bound_range_exceeded(),
        );
    }
    cases.push(
        Case::preserving_flags(
            "upper field exceeds a small expand-down segment",
            &[0x62, 0x03],
        )
        .segmented_only()
        .initial_registers(&[(Eax, 7), (Ebx, 0xfffc)])
        .segment(Segment::Ds, segment(0, 0x3fff, 0x0d))
        .memory(0xfffc, &(-5i32).to_le_bytes(), ReadOnly)
        .general_protection(0),
    );
    cases
}
test_cases!(segment_faults_follow_each_field_access, segment_ordering());

fn field_wrapping() -> Vec<Case> {
    let mut cases = Vec::new();
    for (word, default16) in [(true, false), (false, true)] {
        let width = if word { 2 } else { 4 };
        let bytes = pair(word, -5, 7);
        let mut code = vec![0x66];
        if !default16 {
            code.push(0x67);
        }
        code.extend([0x62, 0x06]);
        code.extend_from_slice(&((0x10000 - width) as u16).to_le_bytes());
        for present in [false, true] {
            let mut case = Case::preserving_flags(
                format!("upper field wraps at address size, word={word}, present={present}"),
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
    cases.extend([
        Case::preserving_flags(
            "lower word bytes remain consecutive across 64K",
            &[0x66, 0x67, 0x62, 0x07],
        )
        .initial_registers(&[(Eax, 7), (Ebx, 0xffff)])
        .memory(0xffff, &[0xfb, 0xff], ReadOnly)
        .memory(1, &[7, 0], ReadOnly),
        Case::preserving_flags(
            "lower word cannot exceed the segment before comparison",
            &[0x66, 0x67, 0x62, 0x07],
        )
        .segmented_only()
        .initial_registers(&[(Eax, -6i32 as u32), (Ebx, 0xffff)])
        .segment(Segment::Ds, segment(0, 0xffff, 0x05))
        .general_protection(0),
        Case::preserving_flags(
            "upper dword address wraps at 32 bits",
            &absolute_code(false, false, 0, 0xffff_fffc),
        )
        .initial_register(Eax, 7)
        .memory(0xffff_fffc, &(-5i32).to_le_bytes(), ReadOnly)
        .memory(0, &7i32.to_le_bytes(), ReadOnly),
    ]);
    cases
}
test_cases!(
    field_starts_wrap_but_individual_bytes_do_not,
    field_wrapping()
);
