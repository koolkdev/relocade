use super::*;
use crate::support::{
    blocks::BlockModules,
    machine::expected,
    step::{Argument, Event, Input, TestModule},
};
use std::sync::OnceLock;

fn module() -> &'static TestModule {
    static MODULE: OnceLock<TestModule> = OnceLock::new();
    MODULE.get_or_init(|| {
        let compiled = wasm86_x86::compile_real_mode_interrupt().unwrap();
        assert_eq!(compiled.entry, "deliver_interrupt");
        assert_eq!(compiled.execution_profile, Some(ExecutionProfile::Real16));
        wasmparser::Validator::new()
            .validate_all(&compiled.bytes)
            .unwrap();
        TestModule::new(&compiled)
    })
}

fn interrupt_image(vector: u8) -> Image {
    let mut image = image(&[]);
    image.cpu.flags.status_source.kind = 0;
    image.cpu.flags.bytes.if_ = 0x81;
    image.cpu.interrupt_shadow = 0x80;
    image.cpu.segments.cs = cache(Segment::Cs, 0x10);
    image.cpu.segments.ss = cache(Segment::Ss, 0x2000);
    image.cpu.registers.esp = 0xabcd_1006;
    image.map(0, 0x7000, false);
    image.map(0x21, 0x8000, true);
    image.data(0x7000 + u32::from(vector) * 4, &[0xff; 4]);
    image
}

fn input(image: &Image, vector: u8) -> Input {
    let mut input = image.input();
    input.arguments = vec![Argument::I32(i32::from(vector))];
    input
}

fn entered(image: &Image) -> CpuState {
    let mut cpu = image.cpu;
    cpu.eip = 0xffff;
    cpu.segments.cs = cache(Segment::Cs, 0xffff);
    cpu.registers.esp -= 6;
    cpu.flags.bytes.if_ = 0;
    cpu.flags.bytes.tf = 0;
    cpu.flags.bytes.ac = 0;
    cpu.interrupt_shadow = 0;
    cpu
}

fn eligibility(engine: Engine) {
    for (enabled, shadow) in [(0, 0), (0x80, 0), (1, 1), (0xff, 0xff), (0, 1)] {
        let mut image = interrupt_image(0x21);
        image.cpu.flags.bytes.if_ = enabled;
        image.cpu.interrupt_shadow = shadow;
        image.cpu.registers.esp = 1;
        let mut input = input(&image, 0x21);
        input.physical_pages.clear();
        input.mmio_pages = vec![(0, 0x7000), (0x2f, 0x8000)];
        input.observe_mmio = true;
        assert_eq!(
            engine.observe(module(), &input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0200_0000_0000_0000),
                }]
            ),
            "blocked IRQ has no effects, callbacks, or retirement"
        );
    }
}

fn frames(engine: Engine) {
    for vector in [0, 3, 0x21, 255] {
        let image = interrupt_image(vector);
        let cpu = entered(&image);
        assert_eq!(
            engine.observe(module(), &input(&image, vector), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[(0x8000, &[0, 0x10, 0x10, 0, 0xd7, 0x5f])],
                    exit: Exit::Dispatch(0xffff),
                }]
            ),
            "accepted IRQ saves current IP, uses low flag bits, and does not retire"
        );
    }
}

fn vectoring(engine: Engine) {
    let image = interrupt_image(0x21);
    let mut input = input(&image, 0x21);
    input
        .physical_pages
        .retain(|&(page, _, _)| page != 0 && page != 0x21);
    input.mmio_pages = vec![(0, 0x7000), (0x21, 0x8000)];
    input.observe_mmio = true;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu: entered(&image),
            ram: &[(0x8000, &[0, 0x10, 0x10, 0, 0xd7, 0x5f])],
            exit: Exit::Dispatch(0xffff),
        }],
    );
    wanted.events.splice(
        0..0,
        [
            Event::MmioWrite {
                address: 0x21004,
                value: vec![0xd7, 0x5f],
            },
            Event::MmioWrite {
                address: 0x21002,
                value: vec![0x10, 0],
            },
            Event::MmioWrite {
                address: 0x21000,
                value: vec![0, 0x10],
            },
            Event::MmioRead {
                address: 0x86,
                bytes: 2,
            },
            Event::MmioRead {
                address: 0x84,
                bytes: 2,
            },
        ],
    );
    assert_eq!(engine.observe(module(), &input, 1), wanted);

    let mut image = interrupt_image(3);
    image.cpu.segments.ss = cache(Segment::Ss, 0);
    image.cpu.registers.esp = 0xabcd_0012;
    image.physical_pages.retain(|&(page, _, _)| page != 0);
    image.map(0, 0x7000, true);
    let mut cpu = entered(&image);
    cpu.eip = 0x1000;
    cpu.segments.cs = image.cpu.segments.cs;
    assert_eq!(
        engine.observe(module(), &self::input(&image, 3), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[(0x700c, &[0, 0x10, 0x10, 0, 0xd7, 0x5f])],
                exit: Exit::Dispatch(0x1000),
            }]
        ),
        "stack writes precede an aliased IVT read"
    );
}

fn delivery_fault(engine: Engine) {
    let mut image = interrupt_image(0x21);
    image.cpu.registers.esp = 1;
    let mut input = input(&image, 0x21);
    input.physical_pages.clear();
    input.mmio_pages = vec![(0, 0x7000), (0x2f, 0x8000)];
    input.observe_mmio = true;
    let mut cpu = image.cpu;
    cpu.interrupt_shadow = 0;
    assert_eq!(
        engine.observe(module(), &input, 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::StackFault { error: 0 }
            }]
        ),
        "accepted delivery faults clear inhibition without retirement or memory effects"
    );
}

fn after_sti(engine: Engine, frontend: Frontend) {
    let code = [0xfb, 0x90];
    let mut image = interrupt_image(0x21);
    image.cpu.segments.cs = cache(Segment::Cs, 0);
    image.cpu.flags.bytes.if_ = 0;
    image.cpu.interrupt_shadow = 0;
    image.data(0x3000, &code);
    let mut blocks = BlockModules::default();
    for (index, opcode) in code.into_iter().enumerate() {
        let instruction = [opcode];
        let execution = match frontend {
            Frontend::Block => blocks.get(&image.cpu, &instruction, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        let result = engine.observe(execution, &image.input(), 1);
        let Event::Return { snapshot, .. } = result.events.last().unwrap() else {
            panic!("execution return")
        };
        image.cpu = CpuState::from_bytes(snapshot.cpu[..CpuState::BYTE_LEN].try_into().unwrap());
        assert_eq!(image.cpu.eip, 0x1001 + index as u32);
        assert_eq!(image.cpu.instruction_count, index as u32);
        let (cpu, ram, exit) = if index == 0 {
            (image.cpu, &[][..], Exit::Other(0x0200_0000_0000_0000))
        } else {
            (
                entered(&image),
                &[(0x8000, &[2, 0x10, 0, 0, 0xd7, 0x5f][..])][..],
                Exit::Dispatch(0xffff),
            )
        };
        assert_eq!(
            engine.observe(module(), &input(&image, 0x21), 1),
            expected(&image, &[Step { cpu, ram, exit }])
        );
    }
}
test_frontends!(interrupt_delivery_after_sti, after_sti);

fn checks(engine: Engine) {
    eligibility(engine);
    frames(engine);
    vectoring(engine);
    delivery_fault(engine);
}
#[test]
fn host_interrupt_delivery() {
    checks(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_host_interrupt_delivery() {
    checks(Engine::V8);
}
