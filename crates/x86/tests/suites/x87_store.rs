//! Narrow stores round the current extended value and retain architectural evidence.

#[path = "x87_store/effects.rs"]
mod effects;
#[path = "x87_store/loaded.rs"]
mod loaded;
#[path = "x87_store/range.rs"]
mod range;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{complete_x87, dispatch, set_control, stack_image, status, write_register_bits},
};
use wasm86_x86::{CpuState, SegmentProfile};

const TOP: u64 = 1 << 63;
const PE: u16 = 0x20;
const C1: u16 = 0x200;
const PENDING: u16 = 0x8080;

#[derive(Clone, Copy, Debug)]
struct Format {
    opcode: u8,
    bytes: usize,
    fraction_bits: u32,
    one: u64,
    infinity: u64,
    minimum_exponent: u16,
    maximum_exponent: u16,
}

const FORMATS: [Format; 2] = [
    Format {
        opcode: 0xd9,
        bytes: 4,
        fraction_bits: 23,
        one: 0x3f80_0000,
        infinity: 0x7f80_0000,
        minimum_exponent: 0x3f81,
        maximum_exponent: 0x407e,
    },
    Format {
        opcode: 0xdd,
        bytes: 8,
        fraction_bits: 52,
        one: 0x3ff0_0000_0000_0000,
        infinity: 0x7ff0_0000_0000_0000,
        minimum_exponent: 0x3c01,
        maximum_exponent: 0x43fe,
    },
];

impl Format {
    fn instruction(self, address: u32, pop: bool) -> Vec<u8> {
        [
            &[self.opcode, if pop { 0x1d } else { 0x15 }],
            address.to_le_bytes().as_slice(),
        ]
        .concat()
    }

    fn sign(self) -> u64 {
        1 << (self.bytes * 8 - 1)
    }
    fn quiet_bit(self) -> u64 {
        1 << (self.fraction_bits - 1)
    }
    fn unit(self) -> u64 {
        1 << (63 - self.fraction_bits)
    }
    fn minimum_normal(self) -> u64 {
        1 << self.fraction_bits
    }

    fn completed(self, cpu: CpuState, pop: bool) -> CpuState {
        let mut cpu = complete_x87(
            cpu,
            6,
            (u16::from(self.opcode & 7) << 8) | if pop { 0x1d } else { 0x15 },
        );
        cpu.x87.data_offset = 0x4000;
        cpu.x87.data_selector = 0x23;
        cpu
    }
}

#[derive(Debug)]
struct StoreCase {
    source: (u64, u16),
    control: u16,
    empty: bool,
    output: Option<u64>,
    flags: u16,
}

impl StoreCase {
    fn masked(source: (u64, u16), output: u64, flags: u16) -> Self {
        Self {
            source,
            control: 0x037f,
            empty: false,
            output: Some(output),
            flags,
        }
    }
}

fn initial_image(code: &[u8], case: &StoreCase) -> Image {
    let exponent = case.source.1 & 0x7fff;
    let tag = if case.empty {
        3
    } else if case.source.0 == 0 && exponent == 0 {
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

fn check_store(checks: &mut ImageSequences, format: Format, pop: bool, case: StoreCase) {
    let code = [format.instruction(0x4000, pop), vec![0xdf, 0xe0, 0x9b]].concat();
    let image = initial_image(&code, &case);
    let mut stored = format.completed(image.cpu, pop);
    let top = if pop && case.output.is_some() { 0 } else { 7 };
    let status_word = 0x4500 | (top << 11) | case.flags;
    stored.x87.status = status(status_word);
    if pop && case.output.is_some() {
        stored.x87.tag_word = 0xffff;
    }
    let output = case.output.unwrap_or(0).to_le_bytes();
    let writes = case
        .output
        .map(|_| (0x8000, &output[..format.bytes]))
        .into_iter()
        .collect::<Vec<_>>();
    let mut observed = stored;
    observed.eip += 2;
    observed.instruction_count = observed.instruction_count.wrapping_add(1);
    observed.registers.eax = 0x1111_0000 | u32::from(status_word);
    let mut waited = observed;
    let exit = if case.flags & PENDING != 0 {
        Exit::FloatingPoint
    } else {
        waited.eip += 1;
        waited.instruction_count = waited.instruction_count.wrapping_add(1);
        Exit::Dispatch(waited.eip)
    };
    checks.check(
        &format!("store {} bits, pop {pop}: {case:x?}", format.bytes * 8),
        &code,
        &image,
        &[
            Step {
                cpu: stored,
                ram: &writes,
                exit: Exit::Dispatch(stored.eip),
            },
            dispatch(observed),
            Step {
                cpu: waited,
                ram: &[],
                exit,
            },
        ],
    );
}

fn finite_rounding(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        for negative in [false, true] {
            let sign = if negative { format.sign() } else { 0 };
            let se = if negative { 0xbfff } else { 0x3fff };
            // Offsets around a midpoint distinguish direct rounding from an
            // intermediate binary64 rounding, even one bit away from a tie.
            for (tail, nearest, nearest_c1) in [
                (0, 0, false),
                (format.unit() / 2 - 1, 0, false),
                (format.unit() / 2, 0, false),
                (format.unit() / 2 + 1, 1, true),
                (format.unit() * 3 / 2, 2, true),
            ] {
                for rc in 0..4 {
                    let floor = u64::from(tail >= format.unit());
                    let (units, incremented) = if rc == 0 {
                        (nearest, nearest_c1)
                    } else if tail != 0 && ((rc == 1 && negative) || (rc == 2 && !negative)) {
                        (floor + 1, true)
                    } else {
                        (floor, false)
                    };
                    let mut case = StoreCase::masked(
                        (TOP + tail, se),
                        sign | (format.one + units),
                        if tail == 0 { 0 } else { PE } | if incremented { C1 } else { 0 },
                    );
                    case.control |= rc << 10;
                    check_store(&mut checks, format, rc & 1 != 0, case);
                }
            }
            check_store(
                &mut checks,
                format,
                true,
                StoreCase::masked(
                    (u64::MAX, se),
                    sign | (format.one + format.minimum_normal()),
                    PE | C1,
                ),
            );
        }
        for pc in [0, 0x200, 0x300] {
            let mut case = StoreCase::masked(
                (TOP + format.unit() / 2 + 1, 0x3fff),
                format.one + 1,
                PE | C1 | PENDING,
            );
            // PC cannot round the source first; PM=0 still commits store/pop.
            case.control = 0x005f | pc;
            check_store(&mut checks, format, true, case);
        }
    }
}

fn special_values(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for format in FORMATS {
        for (source, output, invalid) in [
            ((0, 0), 0, false),
            ((0, 0x8000), format.sign(), false),
            ((TOP, 0x7fff), format.infinity, false),
            ((TOP, 0xffff), format.sign() | format.infinity, false),
            (
                (0xc123_4567_89ab_cdef, 0xffff),
                format.sign()
                    | format.infinity
                    | (0x4123_4567_89ab_cdef >> (63 - format.fraction_bits)),
                false,
            ),
            (
                (TOP | 1, 0x7fff),
                format.infinity | format.quiet_bit(),
                true,
            ),
            (
                (0x8123_4567_89ab_cdef, 0xffff),
                format.sign()
                    | format.infinity
                    | format.quiet_bit()
                    | (0x0123_4567_89ab_cdef >> (63 - format.fraction_bits)),
                true,
            ),
            (
                (0, 1),
                format.sign() | format.infinity | format.quiet_bit(),
                true,
            ),
            (
                (1, 0x3fff),
                format.sign() | format.infinity | format.quiet_bit(),
                true,
            ),
            (
                (0, 0x7fff),
                format.sign() | format.infinity | format.quiet_bit(),
                true,
            ),
            (
                (0x4000_0000_0000_0000, 0x7fff),
                format.sign() | format.infinity | format.quiet_bit(),
                true,
            ),
        ] {
            for masked in [true, false] {
                let mut case = StoreCase::masked(source, output, u16::from(invalid));
                if !masked {
                    // No lower-priority numerical exception applies to a NaN.
                    case.control = 0x0340;
                    if invalid {
                        case.output = None;
                        case.flags |= PENDING;
                    }
                }
                check_store(&mut checks, format, true, case);
            }
        }
        for source in [(TOP | 1, 0x7fff), (TOP, 0x7ffe), (1, 0)] {
            for masked in [true, false] {
                let mut case = StoreCase::masked(
                    source,
                    format.sign() | format.infinity | format.quiet_bit(),
                    0x41,
                );
                case.empty = true;
                case.control = if masked { 0x0341 } else { 0x0340 };
                if !masked {
                    case.output = None;
                    case.flags |= PENDING;
                }
                check_store(&mut checks, format, true, case);
            }
        }
    }
}

test_frontends!(rounding, finite_rounding);
test_frontends!(special, special_values);
