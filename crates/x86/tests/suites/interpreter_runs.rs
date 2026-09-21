//! Runtime decoding continues between block boundaries.

use crate::support::{
    machine::{expected, Exit, Image, Step},
    step::{Engine, Event, SegmentResolution, TestModule},
};
use wasm86_x86::{
    compile_interpreter, Segment, SegmentAttributes, SegmentDefaultSize, SegmentKind,
    SegmentProfile, StoredSegment,
};

fn check(engine: Engine, module: &TestModule, name: &str, image: &Image, step: Step<'_>) {
    assert_eq!(
        engine.observe(module, &image.input(), 1),
        expected(image, &[step]),
        "{name}, {engine:?}"
    );
}

fn dispatch_boundaries(engine: Engine, module: &TestModule) {
    let image = Image::new(&[
        0xb8, 0x78, 0x56, 0x34, 0x12, // MOV EAX,12345678
        0xb4, 0x9a, // MOV AH,9A
        0x89, 0xc1, // MOV ECX,EAX
        0xeb, 0, 0xf4, // JMP to unsupported successor
    ]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1234_9a78;
    cpu.registers.ecx = 0x1234_9a78;
    cpu.eip = 0x100b;
    cpu.instruction_count = 3;
    check(
        engine,
        module,
        "dependent aliases reach one dispatch and retirement wraps",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(0x100b),
        },
    );

    let image = Image::new(&[
        0xb8, 0xff, 0xff, 0xff, 0xff, // MOV EAX,FFFFFFFF
        0x83, 0xc0, 1, // ADD EAX,1
        0x0f, 0x92, 0xc1, // SETB CL
        0x72, 0, // JB to fallthrough
    ]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0;
    cpu.registers.ecx = 0x2222_2201;
    cpu.flags.status_source.kind = 10;
    cpu.flags.status_source.left = u32::MAX;
    cpu.flags.status_source.right = 1;
    cpu.eip = 0x100d;
    cpu.instruction_count = 3;
    check(
        engine,
        module,
        "lazy arithmetic flags feed later SETcc and branch instructions",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );

    for taken in [false, true] {
        let mut image = Image::empty();
        image.cpu.eip = 0x1ff5;
        image.cpu.flags.status_source.kind = 0;
        image.cpu.flags.bytes.zf = u8::from(taken);
        image.map(1, 0x3000, false);
        image.data(0x3ff5, &[0xb8, 42, 0, 0, 0, 0x0f, 0x84, 0, 0x10, 0, 0]);
        let mut cpu = image.cpu;
        cpu.registers.eax = 42;
        cpu.eip = if taken { 0x3000 } else { 0x2000 };
        cpu.instruction_count = 1;
        check(
            engine,
            module,
            "both conditional outcomes dispatch before fetching an absent successor page",
            &image,
            Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }

    let mut code = vec![0x90; 12_000];
    code.extend_from_slice(&[0xeb, 0]);
    let mut image = Image::new(&code);
    image.map(2, 0x4000, false);
    image.map(3, 0x5000, false);
    let mut cpu = image.cpu;
    cpu.eip = 0x1000 + 12_002;
    cpu.instruction_count = 12_000;
    check(
        engine,
        module,
        "long straight-line execution reaches its explicit boundary",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

fn completed_progress_at_faults(engine: Engine, module: &TestModule) {
    for (name, start, suffix, restart, exit) in [
        (
            "absent next opcode",
            0x1ffb,
            &[][..],
            0x2000,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "absent next ModRM",
            0x1ffa,
            &[0x89][..],
            0x1fff,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "incomplete next immediate",
            0x1ff7,
            &[0xb9, 1, 2, 3][..],
            0x1ffc,
            Exit::PageFault {
                address: 0x2000,
                error: 0x10,
            },
        ),
        (
            "next data access faults",
            0x1000,
            &[0x8b, 0x13][..],
            0x1005,
            Exit::PageFault {
                address: 0x4444_4444,
                error: 0,
            },
        ),
        (
            "next opcode is unsupported",
            0x1000,
            &[0xf4][..],
            0x1005,
            Exit::Other(0x0008_00f4_0000_1005),
        ),
    ] {
        let mut image = Image::empty();
        image.cpu.eip = start;
        image.map(1, 0x3000, false);
        let mut code = vec![0xb8, 42, 0, 0, 0];
        code.extend_from_slice(suffix);
        image.data(0x3000 + (start & 0xfff), &code);
        let mut cpu = image.cpu;
        cpu.registers.eax = 42;
        cpu.eip = restart;
        cpu.instruction_count = 0;
        check(
            engine,
            module,
            name,
            &image,
            Step {
                cpu,
                ram: &[],
                exit,
            },
        );
    }
}

fn live_bytes_and_prefixes(engine: Engine, module: &TestModule) {
    let mut image = Image::new(&[
        0xc6, 0x05, 8, 0x10, 0, 0, 42, // MOV byte [1008],42
        0xb0, 7, // MOV AL,7; preceding store replaces its immediate
        0xeb, 0,
    ]);
    image.map(1, 0x3000, true);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1111_112a;
    cpu.eip = 0x100b;
    cpu.instruction_count = 2;
    check(
        engine,
        module,
        "a completed store changes the next instruction's live bytes",
        &image,
        Step {
            cpu,
            ram: &[(0x3008, &[42])],
            exit: Exit::Dispatch(cpu.eip),
        },
    );

    let mut image = Image::new(&[
        0x66, 0xb8, 0x34, 0x12, // MOV AX,1234
        0xb9, 0x78, 0x56, 0x34, 0x12, // MOV ECX,12345678
        0x64, 0x8b, 0x13, // MOV EDX,FS:[EBX]
        0x8b, 0x03, // MOV EAX,[EBX]
        0xeb, 0,
    ]);
    image.cpu.registers.ebx = 0x8000;
    image.cpu.segments.fs.base = 0x1000;
    image.map(8, 0x5000, false);
    image.map(9, 0x6000, false);
    image.data(0x5000, &[0x11; 4]);
    image.data(0x6000, &[0x22; 4]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1111_1111;
    cpu.registers.ecx = 0x1234_5678;
    cpu.registers.edx = 0x2222_2222;
    cpu.eip = 0x1010;
    cpu.instruction_count = 4;
    check(
        engine,
        module,
        "operand and segment prefixes end with their instruction",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );
}

fn repetition_progress(engine: Engine, module: &TestModule) {
    for count in [0, 2] {
        let mut image = Image::new(&[0xb8, 42, 0, 0, 0, 0xf3, 0xa4, 0x89, 0xcb, 0xeb, 0, 0xf4]);
        image.cpu.flags.bytes.df = 0;
        image.cpu.registers.ecx = count;
        image.cpu.registers.esi = 0x8000;
        image.cpu.registers.edi = 0x9000;
        image.map(8, 0x5000, false);
        image.map(9, 0x6000, true);
        image.data(0x5000, &[0x11, 0x22]);
        let mut cpu = image.cpu;
        cpu.registers.eax = 42;
        cpu.registers.ebx = 0;
        cpu.registers.ecx = 0;
        cpu.registers.esi += count;
        cpu.registers.edi += count;
        cpu.eip = 0x100b;
        cpu.instruction_count = 3;
        check(
            engine,
            module,
            "zero and nonzero REP continue through a successor before dispatch",
            &image,
            Step {
                cpu,
                ram: if count == 0 {
                    &[]
                } else {
                    &[(0x6000, &[0x11, 0x22])]
                },
                exit: Exit::Dispatch(cpu.eip),
            },
        );
    }

    let mut image = Image::new(&[0xb8, 42, 0, 0, 0, 0xf3, 0xa4]);
    image.cpu.flags.bytes.df = 0;
    image.cpu.registers.ecx = 2;
    image.cpu.registers.esi = 0x8fff;
    image.cpu.registers.edi = 0xa000;
    image.map(8, 0x5000, false);
    image.map(10, 0x6000, true);
    image.data(0x5fff, &[0x33]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 42;
    cpu.registers.ecx = 1;
    cpu.registers.esi = 0x9000;
    cpu.registers.edi = 0xa001;
    cpu.eip = 0x1005;
    cpu.instruction_count = 0;
    check(
        engine,
        module,
        "REP fault preserves predecessors and completed elements without dispatch",
        &image,
        Step {
            cpu,
            ram: &[(0x6000, &[0x33])],
            exit: Exit::PageFault {
                address: 0x9000,
                error: 0,
            },
        },
    );
}

fn terminal_segment_load(engine: Engine, module: &TestModule) {
    let image = Image::new(&[0xb8, 0x23, 0, 0, 0, 0x8e, 0xd8, 0xf4]);
    let ds = StoredSegment {
        base: 0x2000,
        ..StoredSegment::flat_data32(0x23)
    };
    let mut input = image.input();
    input.segment_resolutions.push(SegmentResolution {
        segment: Segment::Ds as u32,
        selector: 0x23,
        values: [
            0,
            0,
            ds.base,
            ds.limit,
            u32::from(ds.selector),
            u32::from(ds.attributes.bits()),
        ],
    });
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x23;
    cpu.segments.ds = ds;
    cpu.eip = 0x1007;
    cpu.instruction_count = 1;
    let mut observation = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    observation.events.insert(
        0,
        Event::ResolveSegment {
            segment: Segment::Ds as i32,
            selector: 0x23,
        },
    );
    assert_eq!(
        engine.observe(module, &input, 1),
        observation,
        "a segment load dispatches before executing under incompatible flat assumptions"
    );
}

fn segmented_execution(engine: Engine) {
    let module = TestModule::new(&compile_interpreter(SegmentProfile::Segmented16).unwrap());
    let mut image = Image::new(&[
        0x66, 0xb8, 0x78, 0x56, 0x34, 0x12, 0xb8, 0xcd, 0xab, 0x67, 0x8b, 0x03, 0x8b, 0x07, 0xeb, 0,
    ]);
    image.cpu.segments.cs.attributes = SegmentAttributes::new(
        SegmentKind::Code { readable: true },
        SegmentDefaultSize::Bits16,
    );
    image.cpu.registers.ebx = 0x18000;
    image.map(0x18, 0x5000, false);
    image.map(8, 0x6000, false);
    image.data(0x5000, &[0x11; 2]);
    image.data(0x6000, &[0x22; 2]);
    let mut cpu = image.cpu;
    cpu.registers.eax = 0x1234_2222;
    cpu.eip = 0x1010;
    cpu.instruction_count = 4;
    check(
        engine,
        &module,
        "16-bit code restores operand and address defaults between instructions",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        },
    );

    let module = TestModule::new(&compile_interpreter(SegmentProfile::Segmented32).unwrap());
    let mut image = Image::new(&[0xb8, 42, 0, 0, 0, 0x90]);
    image.cpu.segments.cs.limit = 0x1004;
    let mut cpu = image.cpu;
    cpu.registers.eax = 42;
    cpu.eip = 0x1005;
    cpu.instruction_count = 0;
    check(
        engine,
        &module,
        "the next instruction checks CS after publishing its predecessor",
        &image,
        Step {
            cpu,
            ram: &[],
            exit: Exit::GeneralProtection { error: 0 },
        },
    );
}

fn check_runs(engine: Engine) {
    let module = TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap());
    dispatch_boundaries(engine, &module);
    completed_progress_at_faults(engine, &module);
    live_bytes_and_prefixes(engine, &module);
    repetition_progress(engine, &module);
    terminal_segment_load(engine, &module);
    segmented_execution(engine);
}

#[test]
fn instructions_continue_to_execution_boundaries() {
    check_runs(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_instructions_continue_to_execution_boundaries() {
    check_runs(Engine::V8);
}
