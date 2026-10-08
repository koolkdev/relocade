use super::*;
use crate::support::{
    machine::expected,
    step::{Argument, Event, Observation, TestModule},
};
use std::sync::OnceLock;

fn run() -> &'static TestModule {
    static RUN: OnceLock<TestModule> = OnceLock::new();
    RUN.get_or_init(|| {
        TestModule::new(&wasm86_x86::compile_interpreter(ExecutionProfile::Real16).unwrap())
    })
}

fn irq() -> &'static TestModule {
    static IRQ: OnceLock<TestModule> = OnceLock::new();
    IRQ.get_or_init(|| TestModule::new(&wasm86_x86::compile_real_mode_interrupt().unwrap()))
}

fn stopped(image: &Image, bytes: usize) -> CpuState {
    let mut cpu = retired(image, bytes);
    cpu.halted = 1;
    cpu.interrupt_shadow = 0;
    cpu
}

fn instruction(engine: Engine, frontend: Frontend) {
    for code in [&[0xf4][..], &[0x66, 0x67, 0x64, 0xf4][..]] {
        for enabled in [0x80, 0xff] {
            let mut image = image(code);
            image.cpu.flags.bytes.if_ = enabled;
            image.cpu.interrupt_shadow = 1;
            image.cpu.halted = 0x80;
            sequences(engine, frontend).check(
                "HLT preserves flags, advances EIP, retires once and expires inhibition",
                code,
                &image,
                &[Step {
                    cpu: stopped(&image, code.len()),
                    ram: &[],
                    exit: Exit::Other(0x0400_0000_0000_0000),
                }],
            );
        }
    }
}
test_frontends!(halt_instruction, instruction);

fn boundaries(engine: Engine) {
    let code = [0xfb, 0xf4, 0xf1];
    let mut image = image(&code);
    image.cpu.flags.bytes.if_ = 0;
    let mut cpu = stopped(&image, 2);
    cpu.instruction_count = cpu.instruction_count.wrapping_add(1);
    cpu.flags.bytes.if_ = 1;
    let block = TestModule::new(
        &wasm86_x86::compile_block_from_bytes_with_profile(
            0x1000,
            &code,
            3,
            ExecutionProfile::Real16,
        )
        .unwrap(),
    );
    for module in [&block, run()] {
        assert_eq!(
            engine.observe(module, &image.input(), 2),
            expected(
                &image,
                &[
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Other(0x0400_0000_0000_0000)
                    },
                    Step {
                        cpu,
                        ram: &[],
                        exit: Exit::Other(0x0400_0000_0000_0000)
                    },
                ]
            ),
            "STI; HLT stops before its successor and re-entry does not retire again"
        );
    }

    let code = [0xb8, 0x34, 0x12];
    let mut image = super::image(&code);
    image.cpu.halted = 0x81;
    let block = TestModule::new(
        &wasm86_x86::compile_block_from_bytes_with_profile(
            0x1000,
            &code,
            1,
            ExecutionProfile::Real16,
        )
        .unwrap(),
    );
    let step = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    for module in [&block, step, run()] {
        image.check_unchanged_exit(
            engine,
            module,
            "halted entry has no effects",
            Exit::Other(0x0400_0000_0000_0000),
        );
    }
    image.cpu.eip = 0x10000;
    for module in [step, run()] {
        image.check_unchanged_exit(
            engine,
            module,
            "halted entry precedes a CS fetch fault",
            Exit::Other(0x0400_0000_0000_0000),
        );
    }
    image.cpu.eip = 0x1000;
    let mut input = image.input();
    input.physical_pages.clear();
    input.mmio_pages = vec![(1, 0x3000)];
    input.observe_mmio = true;
    for module in [step, run()] {
        assert_eq!(
            engine.observe(module, &input, 1),
            expected(
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::Other(0x0400_0000_0000_0000),
                }]
            ),
            "halted interpreter does not fetch from a device"
        );
    }
}

fn published_cpu(observation: &Observation) -> CpuState {
    let Event::Return { snapshot, .. } = observation.events.last().unwrap() else {
        panic!("entry return")
    };
    CpuState::from_bytes(snapshot.cpu[..CpuState::BYTE_LEN].try_into().unwrap())
}

fn wake_and_return(engine: Engine) {
    let mut image = image(&[0xfb, 0xf4, 0x90]);
    image.cpu.flags = wasm86_x86::StoredFlags::default();
    image.cpu.segments.ss = cache(Segment::Ss, 0x2000);
    image.cpu.registers.esp = 0xabcd_1006;
    image.map(0, 0x7000, false);
    image.map(0x21, 0x8000, true);
    image.map(0x10, 0x9000, false);
    image.data(0x7084, &[0x20, 0, 0, 0x10]);
    image.data(0x9020, &[0xcf]);
    let result = engine.observe(run(), &image.input(), 1);
    let mut halted = stopped(&image, 2);
    halted.instruction_count = 1;
    halted.flags.bytes.if_ = 1;
    assert_eq!(
        result,
        expected(
            &image,
            &[Step {
                cpu: halted,
                ram: &[],
                exit: Exit::Other(0x0400_0000_0000_0000)
            }]
        )
    );
    image.cpu = published_cpu(&result);

    let mut input = image.input();
    input.arguments = vec![Argument::I32(0x21)];
    let result = engine.observe(irq(), &input, 1);
    let frame = [2, 0x10, 0, 0, 2, 2];
    let mut entered = halted;
    entered.halted = 0;
    entered.eip = 0x20;
    entered.segments.cs = cache(Segment::Cs, 0x1000);
    entered.registers.esp -= 6;
    entered.flags.bytes.if_ = 0;
    assert_eq!(
        result,
        expected(
            &image,
            &[Step {
                cpu: entered,
                ram: &[(0x8000, &frame)],
                exit: Exit::Dispatch(0x20)
            }]
        )
    );
    image.cpu = published_cpu(&result);
    image.data(0x8000, &frame);

    let step = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    let result = engine.observe(step, &image.input(), 1);
    let mut returned = entered;
    returned.eip = 0x1002;
    returned.segments.cs = cache(Segment::Cs, 0);
    returned.registers.esp += 6;
    returned.flags.bytes.if_ = 1;
    returned.instruction_count += 1;
    assert_eq!(
        result,
        expected(
            &image,
            &[Step {
                cpu: returned,
                ram: &[],
                exit: Exit::Dispatch(0x1002)
            }]
        )
    );
    image.cpu = published_cpu(&result);
    let resumed = retired(&image, 1);
    assert_eq!(
        engine.observe(step, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu: resumed,
                ram: &[],
                exit: Exit::Dispatch(0x1003)
            }]
        )
    );
}

fn blocked_and_faulting_wake(engine: Engine) {
    for (enabled, shadow, accepted) in [(0, 0, false), (1, 1, false), (1, 0, true)] {
        let mut image = image(&[]);
        image.cpu.halted = 0x81;
        image.cpu.flags.bytes.if_ = enabled;
        image.cpu.interrupt_shadow = shadow;
        image.cpu.registers.esp = 1;
        let mut input = image.input();
        input.arguments = vec![Argument::I32(0x21)];
        input.observe_mmio = true;
        let mut cpu = image.cpu;
        if accepted {
            cpu.halted = 0;
        }
        assert_eq!(
            engine.observe(irq(), &input, 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: &[],
                    exit: if accepted {
                        Exit::StackFault { error: 0 }
                    } else {
                        Exit::Other(0x0200_0000_0000_0000)
                    },
                }]
            ),
            "only accepted delivery wakes; even a vectoring fault leaves the CPU active"
        );
    }
}

fn protected(engine: Engine, frontend: Frontend) {
    use wasm86_x86::SegmentProfile;
    for profile in [
        SegmentProfile::Flat32,
        SegmentProfile::Segmented32,
        SegmentProfile::Segmented16,
    ] {
        let mut cases = ImageSequences::new(engine, frontend, profile);
        for iopl in [0, 3] {
            let code = [0x66, 0x67, 0xf4];
            let mut image = Image::new(&code);
            if profile == SegmentProfile::Segmented16 {
                image.cpu.segments.cs.attributes = SegmentAttributes::from_bits(7);
            }
            image.cpu.halted = 0x81;
            image.cpu.flags.bytes.iopl = iopl;
            cases.check(
                "HLT requires CPL0 independently of IOPL and preserves protected backing",
                &code,
                &image,
                &[Step {
                    cpu: image.cpu,
                    ram: &[],
                    exit: Exit::GeneralProtection { error: 0 },
                }],
            );
        }
    }
}
test_frontends!(halt_protected_privilege, protected);

fn checks(engine: Engine) {
    boundaries(engine);
    wake_and_return(engine);
    blocked_and_faulting_wake(engine);
}
#[test]
fn halt_and_wake() {
    checks(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_halt_and_wake() {
    checks(Engine::V8);
}
