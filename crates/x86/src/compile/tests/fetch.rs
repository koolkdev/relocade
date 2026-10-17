use std::sync::OnceLock;

use crate::{
    support::{
        machine::{expected, Exit, Image, Step},
        step::{Engine, TestModule},
    },
    Compiler, ExecutionProfile, SegmentProfile, Segments,
};

fn block(
    image: &Image,
    code: &[u8],
    instructions: u32,
    profile: impl Into<ExecutionProfile>,
) -> TestModule {
    TestModule::new(
        &Compiler::new(profile)
            .compile_block(image.cpu.eip, code, instructions)
            .unwrap(),
    )
}

fn valid_spans(engine: Engine) {
    for profile in [SegmentProfile::Segmented16, SegmentProfile::Segmented32] {
        // MOV AX/EAX,AX/EAX; the unsupported suffix is outside the instruction limit.
        let code = [0x89, 0xc0, 0xff];
        let mut image = Image::new(&code);
        image.cpu.instruction_count = 17;
        image.cpu.segments.cs.big = u8::from(profile == SegmentProfile::Segmented32);
        let module = block(&image, &code, 1, profile);
        image.cpu.segments.cs.readable = 0; // Execute-only code is valid.
        image.cpu.segments.cs.selector = 0x33;
        image.cpu.segments.cs.base = 0x8000;
        image.map(9, 0x3000, false);
        for limit in [0x1001, 0x2000, u32::MAX] {
            image.cpu.segments.cs.limit = limit;
            let mut after = image.cpu;
            after.eip = 0x1002;
            after.instruction_count = 18;
            assert_eq!(
                engine.observe(&module, &image.input(), 1),
                expected(
                    &image,
                    &[Step {
                        cpu: after,
                        ram: &[],
                        exit: Exit::Dispatch(0x1002)
                    }]
                ),
                "{profile:?}, CS limit {limit:x}",
            );
        }
    }

    // A branch ends compilation even when the supplied buffer and instruction limit continue.
    let code = [0xeb, 0xfe, 0xff];
    let mut image = Image::new(&code);
    image.cpu.instruction_count = 17;
    image.cpu.segments.cs.limit = 0x1001;
    let module = block(&image, &code, 10, SegmentProfile::Segmented32);
    let mut after = image.cpu;
    after.instruction_count = 18;
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[],
                exit: Exit::Dispatch(0x1000)
            }]
        ),
    );

    // Full-size protected segments retain the interpreter's wrapping-offset behavior.
    let code = [0xb8, 0x78, 0x56, 0x34, 0x12];
    let mut image = Image::empty();
    image.cpu.eip = u32::MAX - 1;
    image.cpu.instruction_count = 17;
    image.map(0xfffff, 0x3000, false);
    image.map(0, 0x4000, false);
    image.data(0x3ffe, &code[..2]);
    image.data(0x4000, &code[2..]);
    let module = block(&image, &code, 1, SegmentProfile::Segmented32);
    let mut after = image.cpu;
    after.registers.eax = 0x1234_5678;
    after.eip = 3;
    after.instruction_count = 18;
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[],
                exit: Exit::Dispatch(3)
            }]
        ),
    );
    image.cpu.segments.cs.limit = u32::MAX - 1;
    image.check_unchanged_exit(
        engine,
        &module,
        "finite limit rejects offset wrap",
        Exit::Interpret,
    );
}

fn denied_spans(engine: Engine) {
    let code = [0x89, 0xc0];
    for profile in [SegmentProfile::Segmented16, SegmentProfile::Segmented32] {
        let mut image = Image::new(&code);
        image.cpu.instruction_count = 17;
        image.cpu.segments.cs.big = u8::from(profile == SegmentProfile::Segmented32);
        let module = block(&image, &code, 1, profile);
        for (case, limit, usable, executable) in [
            ("last byte outside CS", 0x1000, 1, 1),
            ("unusable CS", u32::MAX, 0, 1),
            ("non-executable CS", u32::MAX, 1, 0),
        ] {
            image.cpu.segments.cs.limit = limit;
            image.cpu.segments.cs.usable = usable;
            image.cpu.segments.cs.executable = executable;
            image.check_unchanged_exit(engine, &module, case, Exit::Interpret);
        }
    }

    // A valid source span does not admit an invalid branch destination.
    let code = [0xeb, 0x10];
    let mut image = Image::new(&code);
    image.cpu.instruction_count = 17;
    image.cpu.segments.cs.limit = 0x1001;
    image.check_unchanged_exit(
        engine,
        &block(&image, &code, 1, SegmentProfile::Segmented32),
        "branch destination remains an architectural check",
        Exit::GeneralProtection { error: 0 },
    );

    // Real16 also admits the last byte at FFFF, but never an encoding across it.
    let code = [0x89, 0xc0];
    for (eip, exit) in [(0xfffe, Exit::Dispatch(0x10000)), (0xffff, Exit::Interpret)] {
        let mut image = Image::empty();
        image.cpu.segments = Segments::real_mode();
        image.cpu.eip = eip;
        image.cpu.instruction_count = 17;
        image.map(15, 0x3000, false);
        image.map(16, 0x4000, false);
        image.data(0x3000 + (eip & 0xfff), &code);
        let module = block(&image, &code, 1, ExecutionProfile::Real16);
        let mut after = image.cpu;
        if matches!(exit, Exit::Dispatch(_)) {
            after.eip = 0x10000;
            after.instruction_count = 18;
        }
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu: after,
                    ram: &[],
                    exit
                }]
            ),
            "Real16 entry {eip:x}",
        );
    }
}

fn precise_fallback(engine: Engine) {
    static INTERPRETER: OnceLock<TestModule> = OnceLock::new();
    let interpreter = INTERPRETER.get_or_init(|| {
        TestModule::new(
            &Compiler::new(SegmentProfile::Segmented32)
                .compile_interpreter()
                .unwrap(),
        )
    });

    // MOV byte [2000],5A; MOV EAX,12345678. Only the final immediate byte exceeds CS.
    let code = [
        0xc6, 0x05, 0x00, 0x20, 0x00, 0x00, 0x5a, 0xb8, 0x78, 0x56, 0x34, 0x12,
    ];
    let mut image = Image::new(&code);
    image.cpu.instruction_count = 41;
    image.cpu.segments.cs.limit = 0x100a;
    image.map(2, 0x4000, true);
    let module = block(&image, &code, 2, SegmentProfile::Segmented32);
    image.check_unchanged_exit(
        engine,
        &module,
        "handoff precedes the store",
        Exit::Interpret,
    );
    let module = module.with_interpreter(interpreter);
    let mut after = image.cpu;
    after.eip = 0x1007;
    after.instruction_count = 42;
    assert_eq!(
        engine.observe(&module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: after,
                ram: &[(0x4000, &[0x5a])],
                exit: Exit::GeneralProtection { error: 0 },
            }]
        ),
    );

    // A data fault in the valid prefix must win over the later invalid fetch.
    let code = [0x8b, 0x02, 0xb8, 0x78, 0x56, 0x34, 0x12];
    let mut image = Image::new(&code);
    image.cpu.instruction_count = 41;
    image.cpu.segments.cs.limit = 0x1005;
    let module = block(&image, &code, 2, SegmentProfile::Segmented32).with_interpreter(interpreter);
    image.check_unchanged_exit(
        engine,
        &module,
        "data fault precedes later CS failure",
        Exit::PageFault {
            address: 0xdead_beef,
            error: 0,
        },
    );

    // Required instruction bytes can also fault before the CS limit is reached.
    let code = [0xb8, 0x78, 0x56, 0x34, 0x12];
    let mut image = Image::empty();
    image.cpu.eip = 0xffe;
    image.cpu.instruction_count = 41;
    image.cpu.segments.cs.limit = 0x1001;
    image.map(0, 0x3000, false);
    image.data(0x3ffe, &code[..2]);
    let module = block(&image, &code, 1, SegmentProfile::Segmented32).with_interpreter(interpreter);
    image.check_unchanged_exit(
        engine,
        &module,
        "fetch page fault precedes later CS failure",
        Exit::PageFault {
            address: 0x1000,
            error: 0x10,
        },
    );
}

#[test]
fn valid_fetch_spans() {
    valid_spans(Engine::Wasmtime);
}

#[test]
fn denied_fetch_spans() {
    denied_spans(Engine::Wasmtime);
}

#[test]
fn fetch_fallback_preserves_execution_and_fault_order() {
    precise_fallback(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_valid_fetch_spans() {
    valid_spans(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_denied_fetch_spans() {
    denied_spans(Engine::V8);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_fetch_fallback_preserves_execution_and_fault_order() {
    precise_fallback(Engine::V8);
}
