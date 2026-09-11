use wasm86_x86::Gpr32::{Eax, Ebx, Edx, Esp};

use crate::support::cases::{test_cases, InstructionCase as Case, Permissions::ReadOnly};

use super::successful_division as division;

struct Source {
    name: &'static str,
    code: &'static [u8],
    bytes: &'static [u8],
    accumulator: u32,
    high: u32,
    quotient: u32,
    remainder: Option<u32>,
}

fn mapped_source(mut case: Case, bytes: &[u8], address: u32, second_frame: Option<u32>) -> Case {
    let offset = (address & 0xfff) as usize;
    let first_len = bytes.len().min(0x1000 - offset);
    let physical = 0x8000 + offset as u32;
    case = case
        .initial_register(Ebx, address)
        .map_page(4, 0x8000, ReadOnly)
        .backing(physical - 1, &[0x5a])
        .backing(physical, &bytes[..first_len]);
    if let Some(frame) = second_frame {
        case = case
            .map_page(5, frame, ReadOnly)
            .backing(frame, &bytes[first_len..])
            .backing(frame + (bytes.len() - first_len) as u32, &[0xa5]);
    }
    case
}

#[rustfmt::skip]
fn readonly_sources() -> Vec<Case> {
    let mut cases = Vec::new();
    for source in [
        Source { name: "DIV byte", code: &[0xf6, 0x33], bytes: &[3],
            accumulator: 0x4433_0101, high: 0xccbb_aa99, quotient: 0x4433_0255, remainder: None },
        Source { name: "IDIV byte", code: &[0xf6, 0x3b], bytes: &[0xf9],
            accumulator: 0x4433_ff9c, high: 0xccbb_aa99, quotient: 0x4433_fe0e, remainder: None },
        Source { name: "DIV word", code: &[0x66, 0xf7, 0x33], bytes: &[3, 0],
            accumulator: 0x4433_0001, high: 0xccbb_0001, quotient: 0x4433_5555, remainder: Some(0xccbb_0002) },
        Source { name: "IDIV word", code: &[0x66, 0xf7, 0x3b], bytes: &[0xf9, 0xff],
            accumulator: 0x4433_ff9c, high: 0xccbb_ffff, quotient: 0x4433_000e, remainder: Some(0xccbb_fffe) },
        Source { name: "DIV dword", code: &[0xf7, 0x33], bytes: &[3, 0, 0, 0],
            accumulator: 1, high: 1, quotient: 0x5555_5555, remainder: Some(2) },
        Source { name: "IDIV dword", code: &[0xf7, 0x3b], bytes: &[0xf9, 0xff, 0xff, 0xff],
            accumulator: 0xffff_ff9c, high: 0xffff_ffff, quotient: 14, remainder: Some(0xffff_fffe) },
    ] {
        for (layout, address, frame) in [
            ("last complete operand in one page", 0x5000 - source.bytes.len() as u32, None),
            ("contiguous split", 0x4fff, Some(0x9000)),
            ("scattered split", 0x4fff, Some(0xa000)),
        ] {
            if source.bytes.len() == 1 && frame.is_some() {
                continue;
            }
            let mut case = division(format!("{}: {layout}", source.name), source.code)
                .register(Eax, source.accumulator, source.quotient);
            case = if let Some(remainder) = source.remainder {
                case.register(Edx, source.high, remainder)
            } else {
                case.initial_register(Edx, source.high)
            };
            cases.push(mapped_source(case, source.bytes, address, frame));
        }
    }
    cases
}

#[rustfmt::skip]
fn address_aliases() -> Vec<Case> {
    vec![
        division("DIV byte captures the full EAX address before replacing AL and AH", &[0xf6, 0x30])
            .register(Eax, 0x8000_4083, 0x8000_0381)
            .memory(0x8000_4082, &[0x5a, 0x80, 0xa5], ReadOnly),
        division("IDIV byte captures the EAX address before writing a negative quotient", &[0xf6, 0x38])
            .register(Eax, 0x8000_4001, 0x8000_0180)
            .memory(0x8000_4000, &[0x5a, 0x80, 0xa5], ReadOnly),
        division("DIV word uses every address bit and preserves both upper halves", &[0x66, 0xf7, 0x30])
            .register(Eax, 0x8000_4000, 0x8000_6aaa).register(Edx, 0xccbb_0001, 0xccbb_0002)
            .memory(0x8000_3fff, &[0x5a, 3, 0, 0xa5], ReadOnly),
        division("DIV dword captures EAX as source address and dividend", &[0xf7, 0x30])
            .register(Eax, 0x4020, 0x1560).register(Edx, 0, 0)
            .memory(0x401f, &[0x5a, 3, 0, 0, 0, 0xa5], ReadOnly),
        division("IDIV dword captures EAX before a negative quotient replaces it", &[0xf7, 0x38])
            .register(Eax, 0x4020, 0xffff_eaa0).register(Edx, 0, 0)
            .memory(0x401f, &[0x5a, 0xfd, 0xff, 0xff, 0xff, 0xa5], ReadOnly),
        division("DIV dword captures the EDX source address before writing the remainder", &[0xf7, 0x32])
            .register(Eax, 0x1234, 0x4020_0000).register(Edx, 0x4020, 0x1234)
            .memory(0x401f, &[0x5a, 0, 0, 1, 0, 0xa5], ReadOnly),
        division("DIV captures old EAX as a wrapping scaled index", &[0xf7, 0x74, 0x84, 0xfc])
            .register(Eax, 0x4000_0001, 0x2000_0000).register(Edx, 0, 1).initial_register(Esp, 0x4010)
            .memory(0x400f, &[0x5a, 2, 0, 0, 0, 0xa5], ReadOnly),
        division("DIV captures old EDX as a wrapping scaled index", &[0xf7, 0x74, 0x94, 0xfc])
            .register(Eax, 1, 0x8000_0002).register(Edx, 0x4000_0001, 1).initial_register(Esp, 0x4010)
            .memory(0x400f, &[0x5a, 0, 0, 0, 0x80, 0xa5], ReadOnly),
    ]
}

fn mapped_divide_errors() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, width, accumulator, high) in [
        ("DIV byte", &[0xf6, 0x33][..], 1, 0x4433_0100, 0xccbb_aa99),
        ("IDIV byte", &[0xf6, 0x3b][..], 1, 0x4433_0080, 0xccbb_aa99),
        (
            "DIV word",
            &[0x66, 0xf7, 0x33][..],
            2,
            0x4433_0000,
            0xccbb_0001,
        ),
        (
            "IDIV word",
            &[0x66, 0xf7, 0x3b][..],
            2,
            0x4433_8000,
            0xccbb_0000,
        ),
        ("DIV dword", &[0xf7, 0x33][..], 4, 0, 1),
        ("IDIV dword", &[0xf7, 0x3b][..], 4, 0x8000_0000, 0),
    ] {
        for (cause, bytes) in [
            ("zero divisor", [0, 0, 0, 0]),
            ("quotient overflow", [1, 0, 0, 0]),
        ] {
            for (layout, address, frame) in [
                ("one page", 0x5000 - width as u32, None),
                ("contiguous split", 0x4fff, Some(0x9000)),
                ("scattered split", 0x4fff, Some(0xa000)),
            ] {
                if width == 1 && frame.is_some() {
                    continue;
                }
                let case =
                    Case::preserving_flags(format!("{name} {cause}: read-only {layout}"), code)
                        .initial_registers(&[(Eax, accumulator), (Edx, high)])
                        .divide_error();
                cases.push(mapped_source(case, &bytes[..width], address, frame));
            }
        }
    }
    cases
}

fn source_faults() -> Vec<Case> {
    let mut cases = Vec::new();
    for (name, code, width, accumulator, high) in [
        ("DIV byte", &[0xf6, 0x33][..], 1, 0x4433_ffff, 0xccbb_aa99),
        ("IDIV byte", &[0xf6, 0x3b][..], 1, 0x4433_8000, 0xccbb_aa99),
        (
            "DIV word",
            &[0x66, 0xf7, 0x33][..],
            2,
            0x4433_ffff,
            0xccbb_ffff,
        ),
        (
            "IDIV word",
            &[0x66, 0xf7, 0x3b][..],
            2,
            0x4433_0000,
            0xccbb_8000,
        ),
        ("DIV dword", &[0xf7, 0x33][..], 4, 0xffff_ffff, 0xffff_ffff),
        ("IDIV dword", &[0xf7, 0x3b][..], 4, 0, 0x8000_0000),
    ] {
        for (dividend, accumulator, high) in [
            ("nonrepresentable quotient", accumulator, high),
            ("zero dividend", 0, 0),
        ] {
            for (layout, address, fault) in [
                ("absent source page", 0x4020, 0x4020),
                ("absent second source page", 0x4fff, 0x5000),
                ("source span cannot wrap", 0xffff_ffff, 0xffff_ffff),
            ] {
                if width == 1 && layout != "absent source page" {
                    continue;
                }
                let mut case =
                    Case::preserving_flags(format!("{name}: {layout}, {dividend}"), code)
                        .initial_registers(&[(Eax, accumulator), (Edx, high), (Ebx, address)])
                        .backing(0x801f, &[0x5a, 0, 0, 0, 0, 0xa5])
                        .backing(0x8ffc, &[0x5a, 0, 0, 0])
                        .backing(0xa000, &[0, 0, 0, 0xa5])
                        .fault(fault, 0);
                if layout == "absent second source page" {
                    case = case.map_page(4, 0x8000, ReadOnly);
                } else if layout == "source span cannot wrap" {
                    case = case
                        .map_page(0xfffff, 0x8000, ReadOnly)
                        .map_page(0, 0xa000, ReadOnly);
                }
                cases.push(case);
            }
        }
    }
    cases
}

test_cases!(readonly_divisors_and_physical_canaries, readonly_sources());
test_cases!(old_registers_supply_every_source_address, address_aliases());
test_cases!(
    readable_divisors_raise_arithmetic_faults_without_writes,
    mapped_divide_errors()
);
test_cases!(
    complete_source_access_precedes_division_checks,
    source_faults()
);
