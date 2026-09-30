//! FILD converts signed integers exactly and shares the x87 push response.

use crate::support::{
    execution::{test_frontends, Frontend, ImageSequences},
    machine::{expected, Exit, Image, Step},
    step::{Engine, TestModule},
    x87::{
        complete_x87, dispatch, real80, set_control, stack_image, status, write_register_bits,
        INDEFINITE,
    },
};
use wasm86_x86::{compile_block_from_bytes, CpuState, SegmentProfile};

#[derive(Clone, Copy, Debug)]
enum Source {
    Word(i16),
    Dword(i32),
    Qword(i64),
}

impl Source {
    fn encoding(self) -> [u8; 2] {
        match self {
            Self::Word(_) => [0xdf, 0x05],
            Self::Dword(_) => [0xdb, 0x05],
            Self::Qword(_) => [0xdf, 0x2d],
        }
    }

    fn bytes(self) -> Vec<u8> {
        match self {
            Self::Word(value) => value.to_le_bytes().to_vec(),
            Self::Dword(value) => value.to_le_bytes().to_vec(),
            Self::Qword(value) => value.to_le_bytes().to_vec(),
        }
    }

    fn instruction(self, address: u32) -> Vec<u8> {
        [self.encoding().as_slice(), &address.to_le_bytes()].concat()
    }

    fn completed_load(self, cpu: CpuState, address: u32, length: u32) -> CpuState {
        let [opcode, modrm] = self.encoding();
        let mut cpu = complete_x87(cpu, length, (u16::from(opcode & 7) << 8) | u16::from(modrm));
        cpu.x87.data_offset = address;
        cpu.x87.data_selector = 0x23;
        cpu
    }
}

fn initial_image(code: &[u8], source: Source, tags: u16) -> Image {
    let mut image = stack_image(code, 0, tags);
    image.cpu.x87.status.precision = 0;
    image.map(4, 0x8000, true);
    image.data(0x8000, &source.bytes());
    image
}

fn check_conversion(checks: &mut ImageSequences, source: Source, value: (u64, u16), control: u16) {
    let code = [
        source.instruction(0x4000),
        vec![0xdb, 0x3d, 0x20, 0x40, 0, 0],
    ]
    .concat();
    let mut image = initial_image(&code, source, 0xffff);
    set_control(&mut image.cpu.x87.control, control);
    image.data(0x801f, &[0xa6; 12]);
    let mut loaded = source.completed_load(image.cpu, 0x4000, 6);
    loaded.x87.status = status(0x7d00);
    loaded.x87.tag_word = if value == (0, 0) { 0x7fff } else { 0x3fff };
    write_register_bits(&mut loaded, 7, value);
    let mut stored = complete_x87(loaded, 6, 0x033d);
    stored.x87.status.top = 0;
    stored.x87.tag_word = 0xffff;
    stored.x87.data_offset = 0x4020;
    let output = real80(value);
    checks.check(
        &format!("FILD {source:?}, control {control:04x}"),
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
    use Source::{Dword, Qword, Word};
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, value) in [
        (Word(0), (0, 0)),
        (Word(1), (0x8000_0000_0000_0000, 0x3fff)),
        (Word(-1), (0x8000_0000_0000_0000, 0xbfff)),
        (Word(i16::MIN), (0x8000_0000_0000_0000, 0xc00e)),
        (Word(i16::MAX), (0xfffe_0000_0000_0000, 0x400d)),
        (Dword(0), (0, 0)),
        (Dword(-1), (0x8000_0000_0000_0000, 0xbfff)),
        (Dword(0x0100_0001), (0x8000_0080_0000_0000, 0x4017)),
        (Dword(i32::MIN), (0x8000_0000_0000_0000, 0xc01e)),
        (Dword(i32::MAX), (0xffff_fffe_0000_0000, 0x401d)),
        (Qword(0), (0, 0)),
        (Qword(1), (0x8000_0000_0000_0000, 0x3fff)),
        (Qword(-1), (0x8000_0000_0000_0000, 0xbfff)),
        (
            Qword(0x0020_0000_0000_0001),
            (0x8000_0000_0000_0400, 0x4034),
        ),
        (
            Qword(-0x0020_0000_0000_0001),
            (0x8000_0000_0000_0400, 0xc034),
        ),
        (Qword(i64::MIN), (0x8000_0000_0000_0000, 0xc03e)),
        (Qword(i64::MIN + 1), (0xffff_ffff_ffff_fffe, 0xc03d)),
        (Qword(i64::MAX), (0xffff_ffff_ffff_fffe, 0x403d)),
    ] {
        check_conversion(&mut checks, source, value, 0x037f);
    }
    for precision in [0, 0x0200, 0x0300] {
        for rounding in [0, 0x0400, 0x0800, 0x0c00] {
            for (source, value) in [
                (Qword(i64::MAX), (0xffff_ffff_ffff_fffe, 0x403d)),
                (Qword(-i64::MAX), (0xffff_ffff_ffff_fffe, 0xc03d)),
            ] {
                // Precision is unmasked: the 64-bit significand remains exact.
                check_conversion(&mut checks, source, value, 0x005f | precision | rounding);
            }
        }
    }
}

fn source_width_and_faults(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    for (source, value) in [
        (Source::Word(i16::MIN), (0x8000_0000_0000_0000, 0xc00e)),
        (Source::Dword(i32::MIN), (0x8000_0000_0000_0000, 0xc01e)),
        (Source::Qword(i64::MIN), (0x8000_0000_0000_0000, 0xc03e)),
    ] {
        let bytes = source.bytes();
        let address = 0x5000 - bytes.len() as u32;
        let code = [vec![0x66], source.instruction(address)].concat();
        let mut image = stack_image(&code, 0, 0xffff);
        image.map(4, 0xf000, false);
        image.data(address + 0xb000, &bytes);
        let mut loaded = source.completed_load(image.cpu, address, 7);
        loaded.x87.status.top = 7;
        loaded.x87.status.c1 = 0;
        loaded.x87.tag_word = 0x3fff;
        write_register_bits(&mut loaded, 7, value);
        checks.check(
            "66 preserves the encoded integer width",
            &code,
            &image,
            &[dispatch(loaded)],
        );

        let code = source.instruction(address + 1);
        let mut image = stack_image(&code, 0, 0);
        image.map(4, 0x8000, false);
        set_control(&mut image.cpu.x87.control, 0x037e);
        for (pending, exit) in [
            (
                false,
                Exit::PageFault {
                    address: 0x5000,
                    error: 0,
                },
            ),
            (true, Exit::FloatingPoint),
        ] {
            image.cpu.x87.status.invalid = u8::from(pending);
            image.cpu.x87.status.error_summary = u8::from(pending);
            image.cpu.x87.status.busy = u8::from(pending);
            checks.check(
                "pending exception and complete operand guard precede stack overflow",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit,
                }],
            );
        }
    }
}

fn stack_overflow(engine: Engine, frontend: Frontend) {
    let mut checks = ImageSequences::new(engine, frontend, SegmentProfile::Flat32);
    let source = Source::Qword(i64::MIN);
    let instruction = source.instruction(0x4000);
    let code = [instruction.clone(), vec![0x9b]].concat();
    let linked = TestModule::new(&compile_block_from_bytes(0x1000, &instruction, 1).unwrap())
        .with_interpreter(TestModule::interpreter());
    for masked in [false, true] {
        let mut image = initial_image(&code, source, 0);
        set_control(
            &mut image.cpu.x87.control,
            if masked { 0x037f } else { 0x037e },
        );
        let mut loaded = source.completed_load(image.cpu, 0x4000, 6);
        loaded.x87.status = status(if masked { 0x7f41 } else { 0xc7c1 });
        if masked {
            loaded.x87.tag_word = 0x8000;
            write_register_bits(&mut loaded, 7, INDEFINITE);
        }
        let mut waited = loaded;
        if masked {
            waited.eip += 1;
            waited.instruction_count = waited.instruction_count.wrapping_add(1);
        }
        checks.check(
            "stack overflow masks control commitment and deferred delivery",
            &code,
            &image,
            &[
                dispatch(loaded),
                Step {
                    cpu: waited,
                    ram: &[],
                    exit: if masked {
                        Exit::Dispatch(waited.eip)
                    } else {
                        Exit::FloatingPoint
                    },
                },
            ],
        );
        if matches!(frontend, Frontend::Block) {
            assert_eq!(
                engine.observe(&linked, &image.input(), 1),
                expected(&image, &[dispatch(loaded)]),
                "the linked interpreter completes the overflowing FILD"
            );
        }
    }
}

test_frontends!(conversion, exact_conversions);
test_frontends!(operand, source_width_and_faults);
test_frontends!(overflow, stack_overflow);
