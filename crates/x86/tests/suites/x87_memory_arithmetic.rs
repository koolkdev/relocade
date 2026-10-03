//! Real memory arithmetic preserves source precision and exception provenance.

#[path = "x87_memory_arithmetic/boundaries.rs"]
mod boundaries;
#[path = "x87_memory_arithmetic/division.rs"]
mod division;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{complete_x87, dispatch, set_control, stack_image, status, write_value, INDEFINITE},
};
use wasm86_x86::{CpuState, SegmentProfile};

const LEADING: u64 = 1 << 63;

#[derive(Clone, Copy, Debug)]
enum Source {
    Single(u32),
    Double(u64),
}

impl Source {
    fn opcode(self) -> u8 {
        match self {
            Self::Single(_) => 0xd8,
            Self::Double(_) => 0xdc,
        }
    }

    fn bytes(self) -> Vec<u8> {
        match self {
            Self::Single(bits) => bits.to_le_bytes().to_vec(),
            Self::Double(bits) => bits.to_le_bytes().to_vec(),
        }
    }

    fn instruction(self, extension: u8, address: u32) -> Vec<u8> {
        [
            vec![self.opcode(), (extension << 3) | 5],
            address.to_le_bytes().to_vec(),
        ]
        .concat()
    }
}

fn initial_image(code: &[u8], source: Source, left: (u64, u16)) -> Image {
    let mut image = stack_image(code, 3, 0xffff);
    image.cpu.x87.status.precision = 0;
    write_value(&mut image.cpu, 3, left);
    image.map(4, 0x8000, false);
    image.data(0x8000, &source.bytes());
    image
}

fn completed_memory(cpu: CpuState, source: Source, extension: u8) -> CpuState {
    let mut cpu = complete_x87(
        cpu,
        6,
        (u16::from(source.opcode() & 7) << 8) | u16::from((extension << 3) | 5),
    );
    cpu.x87.data_offset = 0x4000;
    cpu.x87.data_selector = 0x23;
    cpu.x87.status.c1 = 0;
    cpu
}

struct Case {
    source: Source,
    extension: u8,
    left: (u64, u16),
    control: u16,
    result: Option<(u64, u16)>,
    flags: u16,
    empty: bool,
}

fn check_case(checks: &mut ImageSequences, name: &str, case: Case) {
    let code = [case.source.instruction(case.extension, 0x4000), vec![0x9b]].concat();
    let mut image = initial_image(&code, case.source, case.left);
    set_control(&mut image.cpu.x87.control, case.control);
    if case.empty {
        image.cpu.x87.tag_word = 0xffff;
    }
    let pending = case.flags & !case.control & 0x3f != 0;
    let mut result = completed_memory(image.cpu, case.source, case.extension);
    result.x87.status = status(0x5d00 | case.flags | if pending { 0x8080 } else { 0 });
    if let Some(value) = case.result {
        write_value(&mut result, 3, value);
    }
    let mut waited = result;
    if !pending {
        waited.eip += 1;
        waited.instruction_count = waited.instruction_count.wrapping_add(1);
    }
    checks.check(
        name,
        &code,
        &image,
        &[
            dispatch(result),
            Step {
                cpu: waited,
                ram: &[],
                exit: if pending {
                    Exit::FloatingPoint
                } else {
                    Exit::Dispatch(waited.eip)
                },
            },
        ],
    );
}

fn real_memory_forms(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x4040_0000),
        Source::Double(0x4008_0000_0000_0000),
    ] {
        for (extension, result) in [
            (0, (LEADING, 0x3fff)),               // -2 + 3 = 1
            (4, (0xa000_0000_0000_0000, 0xc001)), // -2 - 3 = -5
            (5, (0xa000_0000_0000_0000, 0x4001)), // 3 - -2 = 5
            (1, (0xc000_0000_0000_0000, 0xc001)), // -2 * 3 = -6
        ] {
            check_case(
                &mut checks,
                "real source and subtraction order",
                Case {
                    source,
                    extension,
                    left: (LEADING, 0xc000),
                    control: 0x037f,
                    result: Some(result),
                    flags: 0,
                    empty: false,
                },
            );
        }
    }
    // A zero partner does not bypass precision control on the other operand.
    for source in [
        Source::Single(0x8000_0000),
        Source::Double(0x8000_0000_0000_0000),
    ] {
        for (control, significand, flags) in [
            (0x007f, LEADING, 0x20),
            (0x087f, LEADING + (1 << 40), 0x220),
        ] {
            check_case(
                &mut checks,
                "zero source retains precision rounding",
                Case {
                    source,
                    extension: 0,
                    left: (LEADING + 1, 0x3fff),
                    control,
                    result: Some((significand, 0x3fff)),
                    flags,
                    empty: false,
                },
            );
        }
        check_case(
            &mut checks,
            "multiplication preserves the memory zero sign",
            Case {
                source,
                extension: 1,
                left: (LEADING, 0x3fff),
                control: 0x037f,
                result: Some((0, 0x8000)),
                flags: 0,
                empty: false,
            },
        );
    }
    // PC rounds the result, not the source: pre-rounding this binary64 value
    // to 24 bits would incorrectly turn the exact cancellation into zero.
    check_case(
        &mut checks,
        "memory source retains all bits before arithmetic",
        Case {
            source: Source::Double(0x3ff0_0000_0000_0001),
            extension: 0,
            left: (LEADING, 0xbfff),
            control: 0x007f,
            result: Some((LEADING, 0x3fcb)),
            flags: 0,
            empty: false,
        },
    );
}

fn source_exceptions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [
        Source::Single(0x7f80_0000),
        Source::Double(0x7ff0_0000_0000_0000),
    ] {
        for (left, result, flags) in [
            ((LEADING, 0x3fff), (LEADING, 0x7fff), 0),
            ((0, 0), INDEFINITE, 1),
        ] {
            check_case(
                &mut checks,
                "memory infinity and invalid zero product",
                Case {
                    source,
                    extension: 1,
                    left,
                    control: 0x037f,
                    result: Some(result),
                    flags,
                    empty: false,
                },
            );
        }
    }
    for (source, expanded) in [
        (Source::Single(1), (LEADING, 0x3f6a)),
        (Source::Double(0x8000_0000_0000_0001), (LEADING, 0xbbcd)),
    ] {
        for control in [0x037f, 0x037d] {
            check_case(
                &mut checks,
                "source denormal survives exact expansion",
                Case {
                    source,
                    extension: 1,
                    left: (LEADING, 0x3fff),
                    control,
                    result: if control == 0x037f {
                        Some(expanded)
                    } else {
                        None
                    },
                    flags: 2,
                    empty: false,
                },
            );
        }
        let quiet_nan = (0xc000_0000_0000_0001, 0xffff);
        check_case(
            &mut checks,
            "NaN response suppresses source denormal",
            Case {
                source,
                extension: 0,
                left: quiet_nan,
                control: 0x037d,
                result: Some(quiet_nan),
                flags: 0,
                empty: false,
            },
        );
    }
    for (source, quieted) in [
        (Source::Single(0x7fbf_ffff), (0xffff_ff00_0000_0000, 0x7fff)),
        (
            Source::Double(0x7ff7_ffff_ffff_ffff),
            (0xffff_ffff_ffff_f800, 0x7fff),
        ),
    ] {
        for control in [0x037f, 0x037e] {
            check_case(
                &mut checks,
                "memory signaling NaN response",
                Case {
                    source,
                    extension: 4,
                    left: (LEADING, 0x3fff),
                    control,
                    result: if control == 0x037f {
                        Some(quieted)
                    } else {
                        None
                    },
                    flags: 1,
                    empty: false,
                },
            );
        }
        let quiet_nan = (0xc000_0000_0000_0001, 0xffff);
        check_case(
            &mut checks,
            "destination QNaN wins before the source SNaN is quieted",
            Case {
                source,
                extension: 5,
                left: quiet_nan,
                control: 0x037f,
                result: Some(quiet_nan),
                flags: 1,
                empty: false,
            },
        );
    }
    for (source, expanded) in [
        (Source::Single(0x7fc0_0001), (0xc000_0100_0000_0000, 0x7fff)),
        (
            Source::Double(0x7ff8_0000_0000_0001),
            (0xc000_0000_0000_0800, 0x7fff),
        ),
    ] {
        check_case(
            &mut checks,
            "source QNaN wins over a destination SNaN",
            Case {
                source,
                extension: 4,
                left: (0xbfff_ffff_ffff_ffff, 0xffff),
                control: 0x037f,
                result: Some(expanded),
                flags: 1,
                empty: false,
            },
        );
    }
}

fn stack_fault_priority(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for source in [Source::Single(1), Source::Double(0x7ff0_0000_0000_0001)] {
        for control in [0x037f, 0x037e, 0x037d] {
            check_case(
                &mut checks,
                "empty destination overrides source exceptions",
                Case {
                    source,
                    extension: 1,
                    left: (LEADING + 1, 0x7ffe),
                    control,
                    result: if control == 0x037e {
                        None
                    } else {
                        Some(INDEFINITE)
                    },
                    flags: 0x41,
                    empty: true,
                },
            );
        }
    }
}

test_frontends!(forms, real_memory_forms);
test_frontends!(operands, source_exceptions);
test_frontends!(stack_priority, stack_fault_priority);
