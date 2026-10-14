//! FBSTP rounds before checking its decimal range and retains the source sign.

#[path = "store/effects.rs"]
mod effects;

use super::*;

const TOP: u64 = 1 << 63;
const PE: u16 = 0x20;
const C1: u16 = 0x200;
const PENDING: u16 = 0x8080;
const INDEFINITE: [u8; 10] = [0, 0, 0, 0, 0, 0, 0, 0xc0, 0xff, 0xff];

#[derive(Debug)]
struct Case {
    source: (u64, u16),
    control: u16,
    empty: bool,
    output: Option<[u8; 10]>,
    flags: u16,
}

impl Case {
    fn masked(source: (u64, u16), output: [u8; 10], flags: u16) -> Self {
        Self {
            source,
            control: 0x037f,
            empty: false,
            output: Some(output),
            flags,
        }
    }
}

fn instruction(address: u32) -> Vec<u8> {
    [&[0xdf, 0x35][..], &address.to_le_bytes()].concat()
}

fn initial_image(code: &[u8], case: &Case) -> Image {
    let exponent = case.source.1 & 0x7fff;
    let tag = if case.empty {
        3
    } else if exponent == 0 && case.source.0 == 0 {
        1
    } else if exponent == 0 || exponent == 0x7fff || case.source.0 & TOP == 0 {
        2
    } else {
        0
    };
    let mut image = image(code, 7, 0x3fff | (tag << 14));
    set_control(&mut image.cpu.x87.control, case.control);
    write_register_bits(&mut image.cpu, 7, case.source);
    image.data(0x8000, &[0xa6; 12]);
    image
}

fn check_store(checks: &mut ImageSequences, case: Case) {
    let code = [instruction(0x4001), vec![0x9b]].concat();
    let image = initial_image(&code, &case);
    let mut stored = completed(image.cpu, 6, 0x0735, 0x4001);
    stored.x87.status = status(
        if case.output.is_some() {
            0x4500
        } else {
            0x7d00
        } | case.flags,
    );
    if case.output.is_some() {
        stored.x87.tag_word = 0xffff;
    }
    let writes = case
        .output
        .as_ref()
        .map(|bcd| (0x8001, bcd.as_slice()))
        .into_iter()
        .collect::<Vec<_>>();
    let mut waited = stored;
    let exit = if case.flags & PENDING != 0 {
        Exit::FloatingPoint
    } else {
        waited.eip += 1;
        waited.instruction_count = waited.instruction_count.wrapping_add(1);
        Exit::Dispatch(waited.eip)
    };
    checks.check(
        &format!("FBSTP {case:x?}"),
        &code,
        &image,
        &[
            Step {
                cpu: stored,
                ram: &writes,
                exit: Exit::Dispatch(stored.eip),
            },
            Step {
                cpu: waited,
                ram: &[],
                exit,
            },
        ],
    );
}

fn signed(mut digits: [u8; 10], negative: bool) -> [u8; 10] {
    digits[9] = if negative { 0x80 } else { 0 };
    digits
}

fn one_digit(digit: u8, negative: bool) -> [u8; 10] {
    signed([digit, 0, 0, 0, 0, 0, 0, 0, 0, 0], negative)
}

fn exact_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, output) in [
        ((0, 0), [0; 10]),
        ((TOP, 0x3fff), one_digit(1, false)),
        (
            (0xdb4d_a5d3_1879_a700, 0x4037),
            [0x78, 0x56, 0x34, 0x12, 0x90, 0x78, 0x56, 0x34, 0x12, 0],
        ),
        (
            (0x8000_0000_0000_0400, 0x4034),
            [0x93, 0x09, 0x74, 0x54, 0x92, 0x19, 0x07, 0x90, 0, 0],
        ),
        ((0xde0b_6b3a_763f_fff0, 0x403a), MAXIMUM),
    ] {
        for negative in [false, true] {
            check_store(
                &mut checks,
                Case::masked(
                    (source.0, source.1 | if negative { 0x8000 } else { 0 }),
                    signed(output, negative),
                    0,
                ),
            );
        }
    }
    for precision in [0, 0x0200, 0x0300] {
        let mut case = Case::masked((0xde0b_6b3a_763f_fff0, 0x403a), MAXIMUM, 0);
        case.control = 0x005f | precision;
        check_store(&mut checks, case);
    }
}

fn rounding_modes(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, floor, nearest) in [
        ((TOP, 0x3ffe), 0, 0), // 0.5
        ((TOP + 1, 0x3ffe), 0, 1),
        ((0xc000_0000_0000_0000, 0x3fff), 1, 2), // 1.5
        ((0xa000_0000_0000_0000, 0x4000), 2, 2), // 2.5
    ] {
        for negative in [false, true] {
            for rc in 0..4 {
                let magnitude = match rc {
                    0 => nearest,
                    1 if negative => floor + 1,
                    2 if !negative => floor + 1,
                    _ => floor,
                };
                let mut case = Case::masked(
                    (source.0, source.1 | if negative { 0x8000 } else { 0 }),
                    one_digit(magnitude, negative),
                    PE | if magnitude > floor { C1 } else { 0 },
                );
                case.control |= rc << 10;
                check_store(&mut checks, case);
            }
        }
    }
    for precision in [0, 0x0200, 0x0300] {
        // Unmasked #P still stores the rounded value and pops before FWAIT faults.
        let mut case = Case::masked(
            (0xc000_0000_0000_0000, 0xbfff),
            one_digit(2, true),
            PE | C1 | PENDING,
        );
        case.control = 0x005f | precision;
        check_store(&mut checks, case);
    }
}

fn rounded_range(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for negative in [false, true] {
        for rc in 0..4 {
            // The maximum is odd. Maximum + 0.5 rounds outside the decimal range
            // for nearest-even and for rounding away from zero.
            let invalid = rc == 0 || (rc == 1 && negative) || (rc == 2 && !negative);
            let mut case = Case::masked(
                (
                    0xde0b_6b3a_763f_fff8,
                    if negative { 0xc03a } else { 0x403a },
                ),
                if invalid {
                    INDEFINITE
                } else {
                    signed(MAXIMUM, negative)
                },
                if invalid { 1 } else { PE },
            );
            case.control |= rc << 10;
            check_store(&mut checks, case);
        }
        // Just below the halfway point still rounds back into range.
        check_store(
            &mut checks,
            Case::masked(
                (
                    0xde0b_6b3a_763f_fff7,
                    if negative { 0xc03a } else { 0x403a },
                ),
                signed(MAXIMUM, negative),
                PE,
            ),
        );
        for masked in [false, true] {
            let mut case = Case::masked(
                (
                    0xde0b_6b3a_7640_0000,
                    if negative { 0xc03a } else { 0x403a },
                ),
                INDEFINITE,
                1,
            );
            case.control = if masked { 0x0341 } else { 0x0340 };
            if !masked {
                case.output = None;
                case.flags |= PENDING;
            }
            check_store(&mut checks, case);
        }
    }
}

fn exceptional_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        (TOP, 0x7fff),
        (TOP, 0xffff), // infinities
        (TOP | 1, 0x7fff),
        (0xc123_4567_89ab_cdef, 0xffff), // SNaN and QNaN
        (1, 0x3fff),
        (0, 1),
        (0, 0x7fff), // unsupported encodings
        (TOP, 0x403f),
        (TOP, 0x7ffe), // 2^64 and a huge finite value
    ] {
        for masked in [false, true] {
            let mut case = Case::masked(source, INDEFINITE, 1);
            case.control = if masked { 0x0341 } else { 0x0340 };
            if !masked {
                case.output = None;
                case.flags |= PENDING;
            }
            check_store(&mut checks, case);
        }
    }
    for significand in [1, TOP - 1, TOP] {
        for negative in [false, true] {
            for rc in 0..4 {
                let increment = (rc == 1 && negative) || (rc == 2 && !negative);
                let mut case = Case::masked(
                    (significand, if negative { 0x8000 } else { 0 }),
                    one_digit(u8::from(increment), negative),
                    PE | if increment { C1 } else { 0 },
                );
                // Decimal stores do not signal #D, #U, or #O, even when unmasked.
                case.control = 0x0365 | (rc << 10);
                check_store(&mut checks, case);
            }
        }
    }
    for masked in [false, true] {
        let mut case = Case::masked((TOP, 0x3fff), INDEFINITE, 0x41);
        case.empty = true;
        case.control = if masked { 0x0341 } else { 0x0340 };
        if !masked {
            case.output = None;
            case.flags |= PENDING;
        }
        check_store(&mut checks, case);
    }
}

test_frontends!(fbstp_exact, exact_values);
test_frontends!(fbstp_rounding, rounding_modes);
test_frontends!(fbstp_range, rounded_range);
test_frontends!(fbstp_exceptional, exceptional_values);
