//! Narrow real loads expand exactly into the extended register format.

#[path = "x87_load/faults.rs"]
mod faults;

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{Exit, Image, Step},
    step::Engine,
    x87::{
        complete_x87, dispatch, real80, set_control, stack_image, status, write_register_bits,
        INDEFINITE,
    },
};
use wasm86_x86::{CpuState, SegmentProfile};

#[derive(Clone, Copy, Debug)]
enum Source {
    Single(u32),
    Double(u64),
}

impl Source {
    fn opcode(self) -> u8 {
        match self {
            Self::Single(_) => 0xd9,
            Self::Double(_) => 0xdd,
        }
    }

    fn bytes(self) -> Vec<u8> {
        match self {
            Self::Single(bits) => bits.to_le_bytes().to_vec(),
            Self::Double(bits) => bits.to_le_bytes().to_vec(),
        }
    }

    fn instruction(self, address: u32) -> Vec<u8> {
        [&[self.opcode(), 0x05], address.to_le_bytes().as_slice()].concat()
    }
}

struct Conversion {
    source: Source,
    value: (u64, u16),
    tag_word: u16,
    invalid: bool,
    denormal: bool,
}

fn initial_image(code: &[u8], source: Source, tags: u16) -> Image {
    let mut image = stack_image(code, 0, tags);
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, true);
    image.data(0x8000, &source.bytes());
    image
}

fn completed_load(cpu: CpuState, source: Source) -> CpuState {
    let mut cpu = complete_x87(cpu, 6, (u16::from(source.opcode() & 7) << 8) | 5);
    cpu.x87.data_offset = 0x4000;
    cpu.x87.data_selector = 0x23;
    cpu
}

fn check_conversion(checks: &mut ImageSequences, case: Conversion, control: u16) {
    let code = [
        case.source.instruction(0x4000),
        vec![0xdb, 0x3d, 0x20, 0x40, 0, 0],
    ]
    .concat();
    let mut image = initial_image(&code, case.source, 0xffff);
    set_control(&mut image.cpu.x87.control, control);
    image.data(0x801f, &[0xa6; 12]);
    let mut loaded = completed_load(image.cpu, case.source);
    loaded.x87.status = status(0x7d00);
    loaded.x87.status.invalid = u8::from(case.invalid);
    loaded.x87.status.denormal = u8::from(case.denormal);
    loaded.x87.tag_word = case.tag_word;
    write_register_bits(&mut loaded, 7, case.value);
    let mut stored = complete_x87(loaded, 6, 0x033d);
    stored.x87.status.top = 0;
    stored.x87.tag_word = 0xffff;
    stored.x87.data_offset = 0x4020;
    let output = real80(case.value);
    checks.check(
        &format!("FLD {:?}, control {control:04x}", case.source),
        &code,
        &image,
        &[
            dispatch(loaded),
            Step {
                cpu: stored,
                ram: &[(0x8020, &output)],
                exit: Exit::Dispatch(stored.eip),
            },
        ],
    );
}

fn exact_conversions(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    use Source::{Double, Single};
    for (source, value, tag_word, exception) in [
        (Single(0), (0, 0), 0x7fff, 0),
        (Single(0x8000_0000), (0, 0x8000), 0x7fff, 0),
        (
            Single(0x3f80_0001),
            (0x8000_0100_0000_0000, 0x3fff),
            0x3fff,
            0,
        ),
        (
            Single(0xc020_0000),
            (0xa000_0000_0000_0000, 0xc000),
            0x3fff,
            0,
        ),
        (
            Single(0x0080_0000),
            (0x8000_0000_0000_0000, 0x3f81),
            0x3fff,
            0,
        ),
        (
            Single(0x7f7f_ffff),
            (0xffff_ff00_0000_0000, 0x407e),
            0x3fff,
            0,
        ),
        (Single(1), (0x8000_0000_0000_0000, 0x3f6a), 0x3fff, 2),
        (
            Single(0x8000_0001),
            (0x8000_0000_0000_0000, 0xbf6a),
            0x3fff,
            2,
        ),
        (
            Single(0x0040_0001),
            (0x8000_0200_0000_0000, 0x3f80),
            0x3fff,
            2,
        ),
        (
            Single(0x007f_ffff),
            (0xffff_fe00_0000_0000, 0x3f80),
            0x3fff,
            2,
        ),
        (
            Single(0x7f80_0000),
            (0x8000_0000_0000_0000, 0x7fff),
            0xbfff,
            0,
        ),
        (
            Single(0xff80_0000),
            (0x8000_0000_0000_0000, 0xffff),
            0xbfff,
            0,
        ),
        (
            Single(0x7fc1_2345),
            (0xc123_4500_0000_0000, 0x7fff),
            0xbfff,
            0,
        ),
        (
            Single(0x7f81_2345),
            (0xc123_4500_0000_0000, 0x7fff),
            0xbfff,
            1,
        ),
        (
            Single(0xff80_0001),
            (0xc000_0100_0000_0000, 0xffff),
            0xbfff,
            1,
        ),
        (Double(0), (0, 0), 0x7fff, 0),
        (Double(0x8000_0000_0000_0000), (0, 0x8000), 0x7fff, 0),
        (
            Double(0x3ff0_0000_0000_0001),
            (0x8000_0000_0000_0800, 0x3fff),
            0x3fff,
            0,
        ),
        (
            Double(0xc004_0000_0000_0000),
            (0xa000_0000_0000_0000, 0xc000),
            0x3fff,
            0,
        ),
        (
            Double(0x0010_0000_0000_0000),
            (0x8000_0000_0000_0000, 0x3c01),
            0x3fff,
            0,
        ),
        (
            Double(0x7fef_ffff_ffff_ffff),
            (0xffff_ffff_ffff_f800, 0x43fe),
            0x3fff,
            0,
        ),
        (Double(1), (0x8000_0000_0000_0000, 0x3bcd), 0x3fff, 2),
        (
            Double(0x8000_0000_0000_0001),
            (0x8000_0000_0000_0000, 0xbbcd),
            0x3fff,
            2,
        ),
        (
            Double(0x0008_0000_0000_0001),
            (0x8000_0000_0000_1000, 0x3c00),
            0x3fff,
            2,
        ),
        (
            Double(0x000f_ffff_ffff_ffff),
            (0xffff_ffff_ffff_f000, 0x3c00),
            0x3fff,
            2,
        ),
        (
            Double(0x7ff0_0000_0000_0000),
            (0x8000_0000_0000_0000, 0x7fff),
            0xbfff,
            0,
        ),
        (
            Double(0xfff0_0000_0000_0000),
            (0x8000_0000_0000_0000, 0xffff),
            0xbfff,
            0,
        ),
        (
            Double(0x7ff8_1234_5678_9abc),
            (0xc091_a2b3_c4d5_e000, 0x7fff),
            0xbfff,
            0,
        ),
        (
            Double(0x7ff0_1234_5678_9abc),
            (0xc091_a2b3_c4d5_e000, 0x7fff),
            0xbfff,
            1,
        ),
        (
            Double(0xfff0_0000_0000_0001),
            (0xc000_0000_0000_0800, 0xffff),
            0xbfff,
            1,
        ),
    ] {
        check_conversion(
            &mut checks,
            Conversion {
                source,
                value,
                tag_word,
                invalid: exception == 1,
                denormal: exception == 2,
            },
            0x037f,
        );
    }

    let source = Double(1);
    let code = [source.instruction(0x4000), vec![0xdb, 0xe2, 0xd9, 0xc0]].concat();
    let image = initial_image(&code, source, 0xffff);
    let value = (0x8000_0000_0000_0000, 0x3bcd);
    let mut loaded = completed_load(image.cpu, source);
    loaded.x87.status = status(0x7d02);
    loaded.x87.tag_word = 0x3fff;
    write_register_bits(&mut loaded, 7, value);
    let mut cleared = loaded;
    cleared.eip += 2;
    cleared.instruction_count = cleared.instruction_count.wrapping_add(1);
    cleared.x87.status.denormal = 0;
    let mut copied = complete_x87(cleared, 2, 0x01c0);
    copied.x87.status.top = 6;
    copied.x87.tag_word = 0x0fff;
    write_register_bits(&mut copied, 6, value);
    checks.check(
        "a converted subnormal is a normal stack value after clearing DE",
        &code,
        &image,
        &[dispatch(loaded), dispatch(cleared), dispatch(copied)],
    );

    for case in [
        Conversion {
            source: Single(0x3f80_0000),
            value: (0x8000_0000_0000_0000, 0x3fff),
            tag_word: 0x3fff,
            invalid: false,
            denormal: false,
        },
        Conversion {
            source: Single(0x7f80_0001),
            value: (0xc000_0100_0000_0000, 0x7fff),
            tag_word: 0xbfff,
            invalid: true,
            denormal: false,
        },
        Conversion {
            source: Double(1),
            value: (0x8000_0000_0000_0000, 0x3bcd),
            tag_word: 0x3fff,
            invalid: false,
            denormal: true,
        },
    ] {
        let code = case.source.instruction(0x4000);
        let mut image = initial_image(&code, case.source, 0xffff);
        image.cpu.x87.status.invalid = 0x80;
        image.cpu.x87.status.denormal = 0x82;
        image.cpu.x87.status.zero_divide = 0x85;
        image.cpu.x87.status.overflow = 0x86;
        image.cpu.x87.status.underflow = 0x89;
        image.cpu.x87.status.precision = 0x8a;
        image.cpu.x87.status.stack_fault = 0x8d;
        let mut loaded = completed_load(image.cpu, case.source);
        loaded.x87.status.top = 7;
        loaded.x87.status.c1 = 0;
        if case.invalid {
            loaded.x87.status.invalid = 0x81;
        }
        if case.denormal {
            loaded.x87.status.denormal = 0x83;
        }
        loaded.x87.tag_word = case.tag_word;
        write_register_bits(&mut loaded, 7, case.value);
        checks.check(
            &format!(
                "FLD {:?} changes only the operand exception it raises",
                case.source
            ),
            &code,
            &image,
            &[dispatch(loaded)],
        );
    }
}

fn precision_controls_do_not_round_loads(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for precision in [0, 0x0200, 0x0300] {
        for rounding in [0, 0x0400, 0x0800, 0x0c00] {
            for sign in [0_u64, 1 << 63] {
                check_conversion(
                    &mut checks,
                    Conversion {
                        source: Source::Double(sign | 0x3ff0_0000_0000_0001),
                        value: (
                            0x8000_0000_0000_0800,
                            if sign == 0 { 0x3fff } else { 0xbfff },
                        ),
                        tag_word: 0x3fff,
                        invalid: false,
                        denormal: false,
                    },
                    // Unmask precision too: exact FLD cannot signal PE.
                    0x005f | precision | rounding,
                );
            }
        }
    }
}

test_frontends!(conversion, exact_conversions);
test_frontends!(precision_controls, precision_controls_do_not_round_loads);
