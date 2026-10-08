use super::*;
use crate::support::{blocks::BlockModules, machine::expected, step::TestModule};

fn retirement(engine: Engine, frontend: Frontend) {
    let code = [0xfb, 0xfb, 0x90];
    let mut image = image(&code);
    image.cpu.flags.bytes.if_ = 0;
    let mut first = retired(&image, 1);
    first.flags.bytes.if_ = 1;
    first.interrupt_shadow = 1;
    let mut second = first;
    second.eip += 1;
    second.instruction_count = second.instruction_count.wrapping_add(1);
    second.interrupt_shadow = 0;
    let mut third = second;
    third.eip += 1;
    third.instruction_count += 1;
    sequences(engine, frontend).check(
        "only STI changing IF from zero arms the delay",
        &code,
        &image,
        &[
            Step {
                cpu: first,
                ram: &[],
                exit: Exit::Dispatch(first.eip),
            },
            Step {
                cpu: second,
                ram: &[],
                exit: Exit::Dispatch(second.eip),
            },
            Step {
                cpu: third,
                ram: &[],
                exit: Exit::Dispatch(third.eip),
            },
        ],
    );
    for code in [&[0x90][..], &[0xeb, 0][..], &[0xf3, 0xa4][..]] {
        let mut image = super::image(code);
        image.cpu.interrupt_shadow = 1;
        image.cpu.registers.ecx = 0;
        let mut cpu = retired(&image, code.len());
        cpu.interrupt_shadow = 0;
        sequences(engine, frontend).check(
            "successful next instruction expires inhibition",
            code,
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::Dispatch(cpu.eip),
            }],
        );
    }
}
test_frontends!(interrupt_shadow_retirement, retirement);

fn faults(engine: Engine, frontend: Frontend) {
    let mut blocks = BlockModules::default();
    for code in [
        &[0x67, 0xa1, 0, 0, 1, 0][..],
        &[0x0f, 0x0b][..],
        &[0x8e, 0x16, 0xff, 0xff][..],
        &[0x17][..],
    ] {
        let mut image = image(code);
        image.cpu.interrupt_shadow = 1;
        image.cpu.registers.esp = 0xffff;
        let module = match frontend {
            Frontend::Block => blocks.get(&image.cpu, code, 1, ExecutionProfile::Real16),
            Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
        };
        let exit = if code[0] == 0x17 {
            Exit::StackFault { error: 0 }
        } else if code[0] == 0x0f {
            Exit::InvalidOpcode
        } else {
            Exit::GeneralProtection { error: 0 }
        };
        image.check_unchanged_exit(
            engine,
            module,
            "host fault exit preserves the incoming delay",
            exit,
        );
    }
    // Vectoring has begun for INT; a delivery fault must leave the shadow cleared.
    let code = [0xcd, 0x21];
    let mut image = image(&code);
    image.cpu.interrupt_shadow = 1;
    image.cpu.registers.esp = 1;
    let module = match frontend {
        Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
        Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
    };
    let mut cpu = image.cpu;
    cpu.interrupt_shadow = 0;
    assert_eq!(
        engine.observe(module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[],
                exit: Exit::StackFault { error: 0 }
            }]
        )
    );
    let code = [0xf3, 0x67, 0xa4];
    let mut image = super::image(&code);
    image.cpu.interrupt_shadow = 1;
    image.cpu.flags.bytes.df = 0;
    image.cpu.registers.ecx = 2;
    image.cpu.registers.esi = 0xffff;
    image.cpu.registers.edi = 0xfffe;
    image.map(0xf, 0x8000, true);
    image.data(0x8fff, &[0x42]);
    let module = match frontend {
        Frontend::Block => blocks.get(&image.cpu, &code, 1, ExecutionProfile::Real16),
        Frontend::Interpreter => TestModule::interpreter_with_profile(ExecutionProfile::Real16),
    };
    let mut cpu = image.cpu;
    cpu.registers.ecx = 1;
    cpu.registers.esi = 0x10000;
    cpu.registers.edi = 0xffff;
    assert_eq!(
        engine.observe(module, &image.input(), 1),
        expected(
            &image,
            &[Step {
                cpu,
                ram: &[(0x8ffe, &[0x42])],
                exit: Exit::GeneralProtection { error: 0 }
            }]
        ),
        "partial REP progress does not retire the instruction or its shadow"
    );
}
test_frontends!(interrupt_shadow_fault_boundaries, faults);

fn floating_point(engine: Engine, frontend: Frontend) {
    let code = [0xfb, 0x9b];
    for pending in [false, true] {
        let mut image = image(&code);
        image.cpu.flags.bytes.if_ = 0;
        image.cpu.x87 = wasm86_x86::StoredX87::default();
        image.cpu.x87.control.invalid_mask = 0;
        image.cpu.x87.status.invalid = u8::from(pending);
        image.cpu.x87.status.error_summary = u8::from(pending);
        let mut sti = retired(&image, 1);
        sti.flags.bytes.if_ = 1;
        sti.interrupt_shadow = 1;
        let mut wait = sti;
        wait.interrupt_shadow = 0;
        if !pending {
            wait.eip += 1;
            wait.instruction_count += 1;
        }
        sequences(engine, frontend).check(
            "#MF clears inhibition without retiring FWAIT",
            &code,
            &image,
            &[
                Step {
                    cpu: sti,
                    ram: &[],
                    exit: Exit::Dispatch(sti.eip),
                },
                Step {
                    cpu: wait,
                    ram: &[],
                    exit: if pending {
                        Exit::FloatingPoint
                    } else {
                        Exit::Dispatch(wait.eip)
                    },
                },
            ],
        );
    }
}
test_frontends!(interrupt_shadow_floating_point_fault, floating_point);

fn unsupported_and_fetch(engine: Engine) {
    let mut image = image(&[0xf4]);
    image.cpu.interrupt_shadow = 1;
    let module = TestModule::interpreter_with_profile(ExecutionProfile::Real16);
    image.check_unchanged_exit(
        engine,
        module,
        "unsupported host exit does not retire",
        Exit::Other(0x0008_00f4_0000_1000),
    );
    image.cpu.eip = 0x10000;
    image.check_unchanged_exit(
        engine,
        module,
        "fetch fault preserves inhibition until event delivery",
        Exit::GeneralProtection { error: 0 },
    );
}
#[test]
fn interrupt_shadow_host_exits() {
    unsupported_and_fetch(Engine::Wasmtime);
}
#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_interrupt_shadow_host_exits() {
    unsupported_and_fetch(Engine::V8);
}
