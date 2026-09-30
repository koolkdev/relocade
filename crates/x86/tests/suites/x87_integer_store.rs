//! Signed integer stores round once, then apply destination bounds and masks.

#[path = "x87_integer_store/effects.rs"]
mod effects;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{complete_x87, dispatch, real80, set_control, stack_image, status, write_register_bits},
};
use wasm86_x86::{CpuState, SegmentProfile};

const TOP: u64 = 1 << 63;
const PE: u16 = 0x20;
const C1: u16 = 0x200;
const PENDING: u16 = 0x8080;

#[derive(Clone, Copy, Debug)]
struct Form {
    opcode: u8,
    modrm: u8,
    bytes: usize,
    pop: bool,
}

const FORMS: [Form; 5] = [
    Form {
        opcode: 0xdf,
        modrm: 0x15,
        bytes: 2,
        pop: false,
    },
    Form {
        opcode: 0xdb,
        modrm: 0x15,
        bytes: 4,
        pop: false,
    },
    Form {
        opcode: 0xdf,
        modrm: 0x1d,
        bytes: 2,
        pop: true,
    },
    Form {
        opcode: 0xdb,
        modrm: 0x1d,
        bytes: 4,
        pop: true,
    },
    Form {
        opcode: 0xdf,
        modrm: 0x3d,
        bytes: 8,
        pop: true,
    },
];

impl Form {
    fn instruction(self, address: u32) -> Vec<u8> {
        [&[self.opcode, self.modrm], address.to_le_bytes().as_slice()].concat()
    }

    fn completed(self, cpu: CpuState, address: u32, length: u32) -> CpuState {
        let mut cpu = complete_x87(
            cpu,
            length,
            (u16::from(self.opcode & 7) << 8) | u16::from(self.modrm),
        );
        cpu.x87.data_offset = address;
        cpu.x87.data_selector = 0x23;
        cpu
    }

    fn minimum(self) -> i64 {
        match self.bytes {
            2 => i64::from(i16::MIN),
            4 => i64::from(i32::MIN),
            8 => i64::MIN,
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
struct Case {
    source: (u64, u16),
    control: u16,
    empty: bool,
    output: Option<i64>,
    flags: u16,
}

impl Case {
    fn masked(source: (u64, u16), output: i64, flags: u16) -> Self {
        Self {
            source,
            control: 0x037f,
            empty: false,
            output: Some(output),
            flags,
        }
    }
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
    let mut image = stack_image(code, 7, 0x3fff | (tag << 14));
    image.cpu.x87.status.precision = 0;
    set_control(&mut image.cpu.x87.control, case.control);
    write_register_bits(&mut image.cpu, 7, case.source);
    image.map(4, 0x8000, true);
    image.data(0x7fff, &[0xa6; 10]);
    image
}

fn check_store(checks: &mut ImageSequences, form: Form, case: Case) {
    let code = [form.instruction(0x4000), vec![0x9b]].concat();
    let image = initial_image(&code, &case);
    let mut stored = form.completed(image.cpu, 0x4000, 6);
    let popped = form.pop && case.output.is_some();
    stored.x87.status = status(if popped { 0x4500 } else { 0x7d00 } | case.flags);
    if popped {
        stored.x87.tag_word = 0xffff;
    }
    let output = case.output.unwrap_or(0).to_le_bytes();
    let writes = case
        .output
        .map(|_| (0x8000, &output[..form.bytes]))
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
        &format!("{form:?}: {case:x?}"),
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

fn exact_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for form in FORMS {
        for (source, output) in [
            ((0, 0), 0),
            ((0, 0x8000), 0),
            ((TOP, 0x3fff), 1),
            ((TOP, 0xbfff), -1),
        ] {
            check_store(&mut checks, form, Case::masked(source, output, 0));
        }
        let (minimum, maximum) = match form.bytes {
            2 => ((TOP, 0xc00e), (0xfffe_0000_0000_0000, 0x400d)),
            4 => ((TOP, 0xc01e), (0xffff_fffe_0000_0000, 0x401d)),
            8 => ((TOP, 0xc03e), (0xffff_ffff_ffff_fffe, 0x403d)),
            _ => unreachable!(),
        };
        check_store(&mut checks, form, Case::masked(minimum, form.minimum(), 0));
        check_store(
            &mut checks,
            form,
            Case::masked(maximum, -(form.minimum() + 1), 0),
        );
    }
}

fn rounding_modes(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let form = FORMS[3];
    // Literal nearest-even results distinguish both sides of ties at full precision.
    for (source, floor, nearest) in [
        ((TOP, 0x3ffd), 0, 0), // 0.25
        ((u64::MAX, 0x3ffd), 0, 0),
        ((TOP, 0x3ffe), 0, 0), // 0.5
        ((TOP + 1, 0x3ffe), 0, 1),
        ((0xbfff_ffff_ffff_ffff, 0x3fff), 1, 1),
        ((0xc000_0000_0000_0000, 0x3fff), 1, 2), // 1.5
        ((0x9fff_ffff_ffff_ffff, 0x4000), 2, 2),
        ((0xa000_0000_0000_0000, 0x4000), 2, 2), // 2.5
        ((0xa000_0000_0000_0001, 0x4000), 2, 3),
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
                    if negative { -magnitude } else { magnitude },
                    PE | if magnitude > floor { C1 } else { 0 },
                );
                case.control |= rc << 10;
                check_store(&mut checks, form, case);
            }
        }
    }
    for pc in [0, 0x200, 0x300] {
        for (source, output, flags) in [
            ((0xbfff_ffff_ffff_ffff, 0x3fff), 1, PE),
            ((0xc000_0000_0000_0001, 0xbfff), -2, PE | C1),
        ] {
            let mut case = Case::masked(source, output, flags | PENDING);
            case.control = 0x005f | pc;
            check_store(&mut checks, form, case);
        }
    }
}

fn rounded_range(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (form, positive_halfway, negative_halfway) in [
        (
            FORMS[2],
            (0xffff_0000_0000_0000, 0x400d),
            Some((0x8000_8000_0000_0000, 0xc00e)),
        ),
        (
            FORMS[3],
            (0xffff_ffff_0000_0000, 0x401d),
            Some((0x8000_0000_8000_0000, 0xc01e)),
        ),
        (FORMS[4], (u64::MAX, 0x403d), None),
    ] {
        for rc in 0..4 {
            // Positive maximum + 0.5 rounds out of range for nearest/up only.
            let invalid = rc == 0 || rc == 2;
            let mut case = Case::masked(
                positive_halfway,
                if invalid {
                    form.minimum()
                } else {
                    -(form.minimum() + 1)
                },
                if invalid { 1 } else { PE },
            );
            case.control |= rc << 10;
            check_store(&mut checks, form, case);
            if let Some(source) = negative_halfway {
                // Negative minimum - 0.5 rounds to the even minimum except for down.
                let mut case = Case::masked(source, form.minimum(), if rc == 1 { 1 } else { PE });
                case.control |= rc << 10;
                check_store(&mut checks, form, case);
            }
        }
        let minimum_exponent = match form.bytes {
            2 => 0x400e,
            4 => 0x401e,
            _ => 0x403e,
        };
        for source in [
            (TOP, minimum_exponent),
            (
                TOP + (1_u64 << (64 - form.bytes * 8)),
                minimum_exponent | 0x8000,
            ),
        ] {
            for masked in [false, true] {
                let mut case = Case::masked(source, form.minimum(), 1);
                case.control = if masked { 0x0341 } else { 0x0340 };
                if !masked {
                    case.output = None;
                    case.flags |= PENDING;
                }
                check_store(&mut checks, form, case);
            }
        }
    }
    // The nearest representable magnitude below 2^63 is halfway to its integer.
    check_store(
        &mut checks,
        FORMS[4],
        Case::masked((u64::MAX, 0xc03d), i64::MIN, PE | C1),
    );
}

fn special_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let form = FORMS[4];
    for source in [
        (TOP, 0x7fff),
        (TOP, 0xffff),
        (TOP | 1, 0x7fff),
        (0xc123_4567_89ab_cdef, 0xffff),
        (1, 0x3fff),
        (0, 1),
        (0, 0x7fff),
        (TOP, 0x7ffe),
    ] {
        for masked in [false, true] {
            let mut case = Case::masked(source, i64::MIN, 1);
            case.control = if masked { 0x0341 } else { 0x0340 };
            if !masked {
                case.output = None;
                case.flags |= PENDING;
            }
            check_store(&mut checks, form, case);
        }
    }
    for significand in [1, TOP - 1, TOP] {
        for negative in [false, true] {
            for rc in 0..4 {
                let increment = (rc == 1 && negative) || (rc == 2 && !negative);
                let mut case = Case::masked(
                    (significand, if negative { 0x8000 } else { 0 }),
                    if increment {
                        if negative {
                            -1
                        } else {
                            1
                        }
                    } else {
                        0
                    },
                    PE | if increment { C1 } else { 0 },
                );
                // Denormal, underflow and overflow are unmasked; none applies.
                case.control = 0x0365 | (rc << 10);
                check_store(&mut checks, form, case);
            }
        }
    }
    for masked in [false, true] {
        let mut case = Case::masked((TOP, 0x3ffe), i64::MIN, 0x41);
        case.empty = true;
        case.control = if masked { 0x0341 } else { 0x0340 };
        if !masked {
            case.output = None;
            case.flags |= PENDING;
        }
        check_store(&mut checks, form, case);
    }
}

test_frontends!(forms, exact_forms);
test_frontends!(rounding, rounding_modes);
test_frontends!(range, rounded_range);
test_frontends!(special, special_values);
