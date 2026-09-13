use super::data;
use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};
use wasm86_x86::{
    Gpr32::{Eax, Ebp, Ebx, Ecx, Esp},
    Segment, StoredSegment,
};

fn default_segments() -> Vec<Case> {
    [
        ("EBX base selects DS", &[0x8b, 0x03][..], 0x1111_1111),
        ("EBP base selects SS", &[0x8b, 0x45, 0], 0x2222_2222),
        ("ESP base selects SS", &[0x8b, 0x04, 0x24], 0x2222_2222),
        (
            "EBP SIB base selects SS",
            &[0x8b, 0x44, 0x0d, 0],
            0x2222_2222,
        ),
        (
            "EBP index does not select SS",
            &[0x8b, 0x04, 0x2b],
            0x3333_3333,
        ),
        (
            "baseless SIB with EBP index selects DS",
            &[0x8b, 0x04, 0x2d, 0, 0, 0, 0],
            0x1111_1111,
        ),
        (
            "displacement alone selects DS",
            &[0x8b, 0x05, 0x20, 0, 0, 0],
            0x1111_1111,
        ),
        ("moffs selects DS", &[0xa1, 0x20, 0, 0, 0], 0x1111_1111),
    ]
    .into_iter()
    .map(|(name, code, value)| {
        Case::preserving_flags(name, code)
            .segmented_only()
            .segment(Segment::Ds, data(0x4000, 0xff))
            .segment(Segment::Ss, data(0x8000, 0xff))
            .initial_registers(&[(Ebx, 0x20), (Ebp, 0x20), (Esp, 0x20), (Ecx, 0)])
            .register(Eax, 0xdead_beef, value)
            .memory(0x4020, &[0x11; 4], ReadOnly)
            .memory(0x8020, &[0x22; 4], ReadOnly)
            .memory(0x4040, &[0x33; 4], ReadOnly)
    })
    .collect()
}

fn explicit_segments() -> Vec<Case> {
    let mut cases = Vec::new();
    for (prefix, segment) in [
        (0x26, Segment::Es),
        (0x2e, Segment::Cs),
        (0x36, Segment::Ss),
        (0x3e, Segment::Ds),
        (0x64, Segment::Fs),
        (0x65, Segment::Gs),
    ] {
        for suffix in [&[0x8b, 0x45, 0][..], &[0xa1, 0x20, 0, 0, 0]] {
            let code = [&[prefix], suffix].concat();
            let base = if segment == Segment::Cs { 0 } else { 0x8000 };
            let mut case =
                Case::preserving_flags(format!("{segment:?} override {suffix:02x?}"), &code)
                    .initial_register(Ebp, 0x20)
                    .register(Eax, 0, 0x1234_5678)
                    .memory(base + 0x20, &[0x78, 0x56, 0x34, 0x12], ReadOnly);
            if segment != Segment::Cs {
                case = case.segment(segment, data(base, 0xff));
            }
            if matches!(segment, Segment::Ds | Segment::Es | Segment::Ss) {
                case = case.segmented_only();
            }
            cases.push(case);
        }
    }
    cases
}

fn no_access() -> Vec<Case> {
    vec![
        Case::preserving_flags(
            "LEA ignores segment usability, base and limit",
            &[0x64, 0x8d, 0x43, 0x7f],
        )
        .segment(Segment::Fs, StoredSegment::unusable(0x53))
        .initial_register(Ebx, 0xffff_fff0)
        .register(Eax, 1, 0x6f),
        Case::preserving_flags(
            "word LEA returns the offset, without the GS base",
            &[0x65, 0x66, 0x8d, 0x03],
        )
        .segment(Segment::Gs, data(0x8765_0000, 0))
        .initial_register(Ebx, 0x1234_5678)
        .register(Eax, 0xaaaa_bbbb, 0xaaaa_5678),
        Case::preserving_flags(
            "register operand ignores unusable segment override",
            &[0x64, 0x8b, 0xc3],
        )
        .segment(Segment::Fs, StoredSegment::unusable(0x53))
        .initial_register(Ebx, 0x1234_5678)
        .register(Eax, 0, 0x1234_5678),
    ]
}

test_cases!(encoded_base_selects_default_segment, default_segments());
test_cases!(all_six_overrides_select_loaded_caches, explicit_segments());
test_cases!(
    offset_calculations_and_registers_need_no_segment_access,
    no_access()
);
