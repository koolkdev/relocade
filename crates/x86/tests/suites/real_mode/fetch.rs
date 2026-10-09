use super::*;
use crate::support::{
    blocks::BlockModules,
    machine::expected,
    step::{DeviceUpdate, Event, TestModule},
};
use std::sync::OnceLock;
use wasm86_x86::compile_interpreter;

fn availability(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for (code, exit) in [
        (&[0x63, 0x07][..], Exit::InvalidOpcode),
        (&[0x0f, 0x02, 0x07][..], Exit::InvalidOpcode),
        (&[0x0f, 0x03, 0x07][..], Exit::InvalidOpcode),
        (&[0x0f, 0x00, 0x27][..], Exit::InvalidOpcode),
        (&[0x0f, 0x00, 0x2f][..], Exit::InvalidOpcode),
    ] {
        let image = image(code);
        let module = match frontend {
            // A terminal mode restriction must stop decoding despite the larger limit.
            Frontend::Block => blocks.get(&image.cpu, code, 8, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        image.check_unchanged_exit(
            engine,
            module,
            "mode rejection precedes operands and stack",
            exit,
        );
    }
}
test_frontends!(mode_availability, availability);

fn fetches(engine: Engine) {
    let module = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    for (start, code, exit) in [
        (0x2000, &[][..], Exit::Other(0x0008_00ff_0000_2000)),
        (0xffff, &[0xb8][..], Exit::GeneralProtection { error: 0 }),
    ] {
        let mut image = image(&[]);
        image.cpu.eip = start;
        if !code.is_empty() {
            image.map(start >> 12, 0x3000, false);
            image.data(0x3000 + (start & 0xfff), code);
        }
        image.check_unchanged_exit(
            engine,
            module,
            "exact fetch checks CS and reads mapped bytes or physical holes",
            exit,
        );
    }
    let mut crossing = image(&[]);
    crossing.cpu.eip = 0x1fff;
    crossing.map(1, 0x3000, false);
    crossing.data(0x3fff, &[0xb8]);
    let mut cpu = retired(&crossing, 3);
    cpu.registers.eax = 0x1111_ffff;
    assert_eq!(
        engine.observe(module, &crossing.input(), 1),
        expected(
            &crossing,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip)
            }]
        )
    );
    let mut image = image(&[0xea, 0x34, 0x12, 0xff, 0xff]);
    let mut cpu = retired(&image, 5);
    cpu.eip = 0x1234;
    cpu.segments.cs = cache(Segment::Cs, 0xffff);
    assert_eq!(
        engine.observe(module, &image.input(), 2),
        expected(
            &image,
            &[
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Dispatch(cpu.eip)
                },
                Step {
                    cpu,
                    ram: &[],
                    exit: Exit::Other(0x0008_00ff_0000_1234)
                },
            ]
        )
    );

    // A real code base participates in fetch translation, including above 1 MiB.
    image = super::image(&[]);
    image.cpu.segments.cs = cache(Segment::Cs, 0xffff);
    image.cpu.eip = 0x20;
    image.map(0x100, 0x8000, false);
    image.data(0x8010, &[0x90]);
    let cpu = retired(&image, 1);
    assert_eq!(
        engine.observe(module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip)
            },]
        )
    );
}

fn segment_fetch_bounds(engine: Engine) {
    let module = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    for eip in [0x10000, 0x8000_0000, u32::MAX] {
        let mut image = image(&[]);
        image.cpu.segments.cs = cache(Segment::Cs, 0xffff);
        image.cpu.eip = eip;
        let mut input = image.input();
        input.mmio_pages = vec![(0, 0x8000)];
        input.observe_mmio = true;
        assert_eq!(
            engine.observe(module, &input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }]
            ),
            "invalid EIP faults without a physical-table trap or device access",
        );
    }
    // The speculative window crosses CS, but this one-byte instruction fits.
    let mut image = image(&[]);
    image.cpu.segments.cs = cache(Segment::Cs, 0xffff);
    image.cpu.eip = 0xfffe;
    image.map(0x10f, 0x8000, false);
    image.data(0x8fee, &[0x90]);
    let mut input = image.input();
    input.observe_mmio = true;
    assert_eq!(
        engine.observe(module, &input, 1),
        expected(
            &image,
            &[Step {
                cpu: retired(&image, 1),
                ram: &[],
                exit: Exit::Dispatch(0xffff),
            }]
        ),
    );
}

fn run_module() -> &'static TestModule {
    static RUN: OnceLock<TestModule> = OnceLock::new();
    RUN.get_or_init(|| TestModule::new(&compile_interpreter(ExecutionProfile::Real16).unwrap()))
}

fn runs(engine: Engine) {
    let module = run_module();
    for (code, exit, next) in [
        (
            &[0xb8, 0x34, 0x12, 0x8e, 0xd8, 0xf4][..],
            Exit::Dispatch(0x1005),
            0x1005,
        ),
        (
            &[0xb8, 0x34, 0x12, 0x63, 0x07][..],
            Exit::InvalidOpcode,
            0x1003,
        ),
    ] {
        let image = image(code);
        let mut cpu = retired(&image, 3);
        cpu.registers.eax = 0x1111_1234;
        cpu.eip = next;
        if matches!(exit, Exit::Dispatch(_)) {
            cpu.segments.ds = cache(Segment::Ds, 0x1234);
            cpu.instruction_count = 1;
        }
        assert_eq!(
            engine.observe(module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit
                },]
            )
        );
    }
    let code = [0xb8, 0x34, 0x12, 0xa1, 0, 0x20, 0xeb, 0];
    let image = image(&code);
    let mut cpu = retired(&image, code.len());
    cpu.registers.eax = 0x1111_ffff;
    cpu.instruction_count = 2;
    assert_eq!(
        engine.observe(module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip)
            }]
        )
    );
}

fn fetch_callbacks(engine: Engine, mmio: bool) {
    let module = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    for (code, start, fetched, exit) in [
        (&[0x90][..], 0x1000, 1, Exit::Dispatch(0x1001)),
        (&[0xb8, 0x78, 0x56][..], 0x1000, 3, Exit::Dispatch(0x1003)),
        (&[0xa1, 0, 0x20][..], 0x1000, 3, Exit::Dispatch(0x1003)),
        (
            &[0xa1, 0xff, 0xff][..],
            0x1000,
            3,
            Exit::GeneralProtection { error: 0 },
        ),
        (
            &[0xc7, 0xc8, 0, 0][..],
            0x1000,
            2,
            Exit::Other(0x0008_00c7_0000_1000),
        ),
        (&[0xb8][..], 0xffff, 1, Exit::GeneralProtection { error: 0 }),
        (
            &[0x66; 16][..],
            0x1000,
            15,
            Exit::GeneralProtection { error: 0 },
        ),
    ] {
        let mut image = image(&[]);
        image.cpu.eip = start;
        image.map(start >> 12, 0x3000, false);
        image.data(0x3000 + (start & 0xfff), code);
        let mut input = image.input();
        if mmio {
            input.mmio_pages.push((start >> 12, 0x3000));
        }
        input.observe_mmio = true;
        let mut observed = engine.observe(module, &input, 1);
        let transfers: Vec<_> = (0..if mmio { fetched } else { 0 })
            .map(|offset| Event::MmioRead {
                address: start + offset,
                bytes: 1,
            })
            .collect();
        let mut cpu = image.cpu;
        if let Exit::Dispatch(next) = exit {
            cpu = retired(&image, code.len());
            assert_eq!(cpu.eip, next);
            if code[0] == 0xb8 {
                cpu.registers.eax = 0x1111_5678;
            }
            if code[0] == 0xa1 {
                cpu.registers.eax = 0x1111_ffff;
            }
        }
        assert_eq!(
            observed.events.drain(..transfers.len()).collect::<Vec<_>>(),
            transfers,
            "fetch effects for {code:02x?}"
        );
        assert_eq!(
            observed,
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit
                }]
            ),
            "no extra fetch or data callback for {code:02x?}"
        );
    }
}

fn live_fetch(engine: Engine) {
    let mut last_byte = image(&[]);
    last_byte.cpu.eip = 0x1fff;
    last_byte.data(0x3fff, &[0x90]);
    let mut input = last_byte.input();
    input.mmio_pages = vec![(2, 0x8000)];
    input.observe_mmio = true;
    assert_eq!(
        engine.observe(
            TestModule::interpreter_with_profile(ExecutionProfile::Real16),
            &input,
            1
        ),
        expected(
            &last_byte,
            &[Step {
                cpu: retired(&last_byte, 1),
                ram: &[],
                exit: Exit::Dispatch(0x2000)
            }]
        )
    );

    // A device write changes the code mapping before the next instruction.
    let mut image = image(&[0xa2, 0, 0x20, 0xb8, 0x11, 0x11, 0xeb, 0]);
    image.data(0x9003, &[0xb8, 0x78, 0x56, 0xeb, 0]);
    let mut input = image.input();
    input.mmio_pages = vec![(2, 0x6000)];
    input.observe_mmio = true;
    input.mmio_updates = vec![DeviceUpdate {
        map: vec![(8, vec![2, 0, 0, 0, 0, 0x90, 0, 0])],
        ..DeviceUpdate::default()
    }];
    let mut cpu = retired(&image, 8);
    cpu.registers.eax = 0x1111_5678;
    cpu.instruction_count = 2;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[(0x6000, &[0x11])],
            exit: Exit::Dispatch(0x1008),
        }],
    );
    wanted.events.insert(
        0,
        Event::MmioWrite {
            address: 0x2000,
            value: vec![0x11],
        },
    );
    assert_eq!(engine.observe(run_module(), &input, 1), wanted);
}

#[test]
fn interpreter_fetch_and_run_boundaries() {
    fetches(Engine::Wasmtime);
    segment_fetch_bounds(Engine::Wasmtime);
    runs(Engine::Wasmtime);
    fetch_callbacks(Engine::Wasmtime, false);
    fetch_callbacks(Engine::Wasmtime, true);
    live_fetch(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interpreter_fetch_and_run_boundaries() {
    fetches(Engine::V8);
    segment_fetch_bounds(Engine::V8);
    runs(Engine::V8);
    fetch_callbacks(Engine::V8, false);
    fetch_callbacks(Engine::V8, true);
    live_fetch(Engine::V8);
}
