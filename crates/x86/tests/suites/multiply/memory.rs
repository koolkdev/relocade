use wasm86_x86::Gpr32::{Eax, Ebx, Edx, Esp};

use crate::support::cases::{
    test_cases,
    FlagExpectation::{self, Clear, Set},
    InstructionCase as Case,
    Permissions::ReadOnly,
};

use super::product_flags;

struct Source {
    name: &'static str,
    code: &'static [u8],
    bytes: &'static [u8],
    accumulator: u32,
    low: u32,
    high: Option<u32>,
    overflow: FlagExpectation,
}

#[rustfmt::skip]
fn readonly_sources() -> Vec<Case> {
    let mut cases = Vec::new();
    for source in [
        Source { name: "MUL byte", code: &[0xf6, 0x23], bytes: &[2],
            accumulator: 0x4433_22f0, low: 0x4433_01e0, high: None, overflow: Set },
        Source { name: "IMUL byte", code: &[0xf6, 0x2b], bytes: &[2],
            accumulator: 0x4433_22f0, low: 0x4433_ffe0, high: None, overflow: Clear },
        Source { name: "MUL word", code: &[0x66, 0xf7, 0x23], bytes: &[2, 0],
            accumulator: 0x4433_8000, low: 0x4433_0000, high: Some(0xccbb_0001), overflow: Set },
        Source { name: "IMUL word", code: &[0x66, 0xf7, 0x2b], bytes: &[2, 0],
            accumulator: 0x4433_8000, low: 0x4433_0000, high: Some(0xccbb_ffff), overflow: Set },
        Source { name: "MUL dword", code: &[0xf7, 0x23], bytes: &[2, 0, 0, 0],
            accumulator: 0x8000_0000, low: 0, high: Some(1), overflow: Set },
        Source { name: "IMUL dword", code: &[0xf7, 0x2b], bytes: &[2, 0, 0, 0],
            accumulator: 0x8000_0000, low: 0, high: Some(0xffff_ffff), overflow: Set },
        Source { name: "IMUL word two operands", code: &[0x66, 0x0f, 0xaf, 0x03], bytes: &[0xfe, 0xff],
            accumulator: 0x4433_0003, low: 0x4433_fffa, high: None, overflow: Clear },
        Source { name: "IMUL dword two operands", code: &[0x0f, 0xaf, 0x03], bytes: &[0xfe, 0xff, 0xff, 0xff],
            accumulator: 3, low: 0xffff_fffa, high: None, overflow: Clear },
        Source { name: "IMUL word wide immediate", code: &[0x66, 0x69, 0x03, 0xfe, 0xff], bytes: &[3, 0],
            accumulator: 0x4433_dead, low: 0x4433_fffa, high: None, overflow: Clear },
        Source { name: "IMUL dword wide immediate", code: &[0x69, 0x03, 0xfe, 0xff, 0xff, 0xff], bytes: &[3, 0, 0, 0],
            accumulator: 0x4433_dead, low: 0xffff_fffa, high: None, overflow: Clear },
        Source { name: "IMUL word signed byte immediate", code: &[0x66, 0x6b, 0x03, 0x80], bytes: &[0xff, 0xff],
            accumulator: 0x4433_dead, low: 0x4433_0080, high: None, overflow: Clear },
        Source { name: "IMUL dword signed byte immediate", code: &[0x6b, 0x03, 0x80], bytes: &[0xff, 0xff, 0xff, 0xff],
            accumulator: 0x4433_dead, low: 0x80, high: None, overflow: Clear },
    ] {
        for (layout, address, second_frame) in [
            ("last complete operand in one page", 0x5000 - source.bytes.len() as u32, None),
            ("contiguous split", 0x4fff, Some(0x9000)),
            ("scattered split", 0x4fff, Some(0xa000)),
        ] {
            if source.bytes.len() == 1 && second_frame.is_some() {
                continue;
            }
            let offset = (address & 0xfff) as usize;
            let first_len = source.bytes.len().min(0x1000 - offset);
            let physical = 0x8000 + offset as u32;
            let mut case = Case::replacing_flags(format!("{}: {layout}", source.name), source.code, product_flags(source.overflow))
                .register(Eax, source.accumulator, source.low)
                .initial_register(Ebx, address)
                .map_page(4, 0x8000, ReadOnly)
                .backing(physical - 1, &[0x5a])
                .backing(physical, &source.bytes[..first_len]);
            case = if let Some(high) = source.high {
                case.register(Edx, 0xccbb_aa99, high)
            } else {
                case.initial_register(Edx, 0xccbb_aa99)
            };
            if let Some(frame) = second_frame {
                case = case.map_page(5, frame, ReadOnly)
                    .backing(frame, &source.bytes[first_len..])
                    .backing(frame + (source.bytes.len() - first_len) as u32, &[0xa5]);
            }
            cases.push(case);
        }
    }
    cases
}

#[rustfmt::skip]
fn address_aliases() -> Vec<Case> {
    vec![
        Case::replacing_flags("MUL byte captures the EAX address before replacing AX", &[0xf6, 0x20], product_flags(Set))
            .register(Eax, 0x4083, 0x0106)
            .memory(0x4082, &[0x5a, 2, 0xa5], ReadOnly),
        Case::replacing_flags("MUL dword captures the EAX address before replacing both halves", &[0xf7, 0x20], product_flags(Clear))
            .register(Eax, 0x4010, 0x8020).register(Edx, 0xccbb_aa99, 0)
            .memory(0x400f, &[0x5a, 2, 0, 0, 0, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL dword captures its EDX source address", &[0xf7, 0x2a], product_flags(Clear))
            .register(Eax, 0xffff_fffd, 3).register(Edx, 0x4020, 0)
            .memory(0x401f, &[0x5a, 0xff, 0xff, 0xff, 0xff, 0xa5], ReadOnly),
        Case::replacing_flags("MUL word uses all address bits and preserves both upper halves", &[0x66, 0xf7, 0x20], product_flags(Clear))
            .register(Eax, 0x8000_4000, 0x8000_c000).register(Edx, 0xccbb_aa99, 0xccbb_0000)
            .memory(0x8000_3fff, &[0x5a, 3, 0, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL two operands captures EDX as both old destination and address", &[0x0f, 0xaf, 0x12], product_flags(Clear))
            .register(Edx, 0x4020, 0xc060)
            .memory(0x401f, &[0x5a, 3, 0, 0, 0, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL wide immediate reads EAX only as the source address", &[0x69, 0x00, 0xfe, 0xff, 0xff, 0xff], product_flags(Clear))
            .register(Eax, 0x4020, 0xffff_fffa)
            .memory(0x401f, &[0x5a, 3, 0, 0, 0, 0xa5], ReadOnly),
        Case::replacing_flags("IMUL signed immediate captures old ESP and wrapping scaled EAX", &[0x6b, 0x64, 0x84, 0xfc, 0xfe], product_flags(Clear))
            .register(Esp, 0x4010, 6).initial_register(Eax, 0x4000_0001)
            .memory(0x400f, &[0x5a, 0xfd, 0xff, 0xff, 0xff, 0xa5], ReadOnly),
        Case::replacing_flags("MUL captures old EDX as a wrapping scaled index", &[0xf7, 0x64, 0x94, 0xfc], product_flags(Clear))
            .register(Eax, 3, 6).register(Edx, 0x4000_0001, 0).initial_register(Esp, 0x4010)
            .memory(0x400f, &[0x5a, 2, 0, 0, 0, 0xa5], ReadOnly),
    ]
}

fn source_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, width) in [
        ("MUL byte zero accumulator", &[0xf6, 0x23][..], 1),
        ("IMUL byte zero accumulator", &[0xf6, 0x2b][..], 1),
        ("MUL word zero accumulator", &[0x66, 0xf7, 0x23][..], 2),
        ("IMUL word zero accumulator", &[0x66, 0xf7, 0x2b][..], 2),
        ("MUL dword zero accumulator", &[0xf7, 0x23][..], 4),
        ("IMUL dword zero accumulator", &[0xf7, 0x2b][..], 4),
        ("IMUL word two operands", &[0x66, 0x0f, 0xaf, 0x03][..], 2),
        ("IMUL dword two operands", &[0x0f, 0xaf, 0x03][..], 4),
        (
            "IMUL word wide immediate zero",
            &[0x66, 0x69, 0x03, 0, 0][..],
            2,
        ),
        (
            "IMUL dword wide immediate zero",
            &[0x69, 0x03, 0, 0, 0, 0][..],
            4,
        ),
        (
            "IMUL word byte immediate zero",
            &[0x66, 0x6b, 0x03, 0][..],
            2,
        ),
        ("IMUL dword byte immediate zero", &[0x6b, 0x03, 0][..], 4),
    ] {
        for (layout, address, fault) in [
            ("missing source page", 0x4020, 0x4020),
            ("missing second source page", 0x4fff, 0x5000),
            ("source span cannot wrap", 0xffff_ffff, 0xffff_ffff),
        ] {
            if width == 1 && layout != "missing source page" {
                continue;
            }
            let mut case = Case::preserving_flags(format!("{name}: {layout}"), code)
                .initial_registers(&[(Eax, 0), (Edx, 0xccbb_aa99), (Ebx, address)])
                .backing(0x801f, &[0x5a, 0x81, 0x80, 0xff, 0xff, 0xa5])
                .backing(0x8ffc, &[0x5a, 1, 0x80, 0xff])
                .backing(0xa000, &[0xff, 0xff, 0xff, 0xa5])
                .fault(fault, 0);
            if layout == "missing second source page" {
                case = case.map_page(4, 0x8000, ReadOnly);
            } else if layout == "source span cannot wrap" {
                case = case
                    .map_page(0xfffff, 0x8000, ReadOnly)
                    .map_page(0, 0xa000, ReadOnly);
            }
            cases.push(case);
        }
    }
    cases
}

test_cases!(
    readonly_sources_keep_their_width_and_physical_canaries,
    readonly_sources()
);
test_cases!(old_registers_supply_source_addresses, address_aliases());
test_cases!(
    complete_source_reads_precede_every_product_or_flag_effect,
    source_faults()
);
