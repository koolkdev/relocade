//! Real Wasm-to-Wasm handoffs use the interpreter's own stopping boundaries.

use std::sync::OnceLock;

use crate::support::{
    machine::{expected, Exit, Step},
    step::{Argument, Engine, Event, Outcome, TestModule},
    x87::{complete_x87, stack_image, write_register_bits},
};
use wasm86_x86::{compile_block_from_bytes, compile_interpreter, CpuState, SegmentProfile};

pub(super) fn interpreter_run() -> &'static TestModule {
    static MODULE: OnceLock<TestModule> = OnceLock::new();
    MODULE.get_or_init(|| TestModule::new(&compile_interpreter(SegmentProfile::Flat32).unwrap()))
}

fn subnormal_loaded(cpu: CpuState) -> CpuState {
    let mut cpu = complete_x87(cpu, 6, 0x0105);
    cpu.x87.data_offset = 0x4000;
    cpu.x87.data_selector = 0x23;
    cpu.x87.status.denormal = 1;
    cpu.x87.status.c1 = 0;
    cpu.x87.status.top = 7;
    cpu.x87.tag_word = 0x3fff;
    write_register_bits(&mut cpu, 7, (0x8000_0000_0000_0000, 0x3f6a));
    cpu
}

fn published_prefix_and_step(engine: Engine) {
    let code = [
        0x8d, 0x40, 1, // LEA EAX,[EAX+1]
        0xa3, 0, 0x40, 0, 0, // MOV [0x4000],EAX
        0xd9, 0x05, 0, 0x40, 0, 0, // FLD dword [0x4000]
        0x8d, 0x40, 1, // The captured suffix is abandoned on handoff.
    ];
    let mut image = stack_image(&code, 0, 0xffff);
    image.cpu.registers.eax = 0;
    image.map(4, 0x8000, true);
    let mut restart = image.cpu;
    restart.eip = 0x1008;
    restart.instruction_count = 1; // Two completed instructions wrap u32::MAX.
    restart.registers.eax = 1;
    for linked in [false, true] {
        let mut module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 4).unwrap());
        let (cpu, exit) = if linked {
            module = module.with_interpreter(TestModule::interpreter());
            let cpu = subnormal_loaded(restart);
            (cpu, Exit::Dispatch(cpu.eip))
        } else {
            (restart, Exit::Interpret)
        };
        let mut wanted = expected(
            &image,
            &[Step {
                cpu,
                ram: &[(0x8000, &1_u32.to_le_bytes())],
                exit,
            }],
        );
        let mut input = image.input();
        // The host's result is opaque even when its bits resemble a guest fault.
        input.dispatch_return = 0x0004_0000_0000_1234;
        let Event::Return { outcome, .. } = wanted.events.last_mut().unwrap() else {
            unreachable!()
        };
        *outcome = Outcome::Returned(vec![Argument::I64(input.dispatch_return)]);
        assert_eq!(
            engine.observe(&module, &input, 1),
            wanted,
            "linked {linked}"
        );
    }
}

fn run_to_branch_or_fault(engine: Engine) {
    let code = [
        0xd9, 0x05, 0, 0x40, 0, 0, // Only FLD belongs to the compiled snapshot.
        0xd9, 0x1d, 0, 0x50, 0, 0, // FSTP dword [0x5000]
        0xeb, 0,    // JMP ends interpreter run.
        0xf1, // Unsupported successor must not be fetched.
    ];
    let module = TestModule::new(&compile_block_from_bytes(0x1000, &code, 1).unwrap())
        .with_interpreter(interpreter_run());
    for destination_present in [false, true] {
        let mut image = stack_image(&code, 0, 0xffff);
        image.map(4, 0x8000, false);
        image.data(0x8000, &1_u32.to_le_bytes());
        let mut cpu = subnormal_loaded(image.cpu);
        let exit = if destination_present {
            image.map(5, 0x9000, true);
            cpu = complete_x87(cpu, 6, 0x011d);
            cpu.x87.data_offset = 0x5000;
            cpu.x87.status.top = 0;
            cpu.x87.tag_word = 0xffff;
            cpu.eip = 0x100e;
            cpu.instruction_count = 2;
            Exit::Dispatch(cpu.eip)
        } else {
            Exit::PageFault {
                address: 0x5000,
                error: 2,
            }
        };
        let bits = 1_u32.to_le_bytes();
        let written = [(0x9000, &bits[..])];
        assert_eq!(
            engine.observe(&module, &image.input(), 1),
            expected(
                &image,
                &[Step {
                    cpu,
                    ram: if destination_present { &written } else { &[] },
                    exit,
                }]
            ),
            "destination present {destination_present}"
        );
    }
}

#[test]
fn handoff_publishes_prefix_and_interpreter_step_restarts_once() {
    published_prefix_and_step(Engine::Wasmtime);
}

#[test]
fn interpreter_run_passes_snapshot_limit_and_stops_at_branch_or_fault() {
    run_to_branch_or_fault(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_linked_interpreter_handoff() {
    published_prefix_and_step(Engine::V8);
    run_to_branch_or_fault(Engine::V8);
}
