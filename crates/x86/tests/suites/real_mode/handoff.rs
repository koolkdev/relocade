//! A handoff interpreter supplies the imports required by its own memory model.

use super::*;
use crate::support::{
    machine::expected,
    step::{Event, TestModule},
};
use relocade_compiler::Program;

#[test]
fn handoff_registers_the_real_mode_interpreters_memory_imports() {
    // The caller only imports interpret; its Real16 callee requires physical memory.
    let mut program = Program::new();
    let interpret = crate::abi::INTERPRET.import(&mut program);
    let entry = program
        .function::<crate::abi::ExecutionSignature>(move |body, ()| interpret.tail_call(body, ()));
    program.export("enter", entry).unwrap();
    let module = TestModule::new(&relocade_x86::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "enter".into(),
        execution_target: Some(crate::Compiler::new(ExecutionProfile::Real16).target()),
    })
    .with_interpreter(TestModule::interpreter_with_profile(
        ExecutionProfile::Real16,
    ));
    let code = [0xa1, 0, 0x20]; // MOV AX,[2000]
    let mut image = image(&code);
    image.data(0x8000, &[0x78, 0x56]);
    let mut input = image.input();
    input.mmio_pages = vec![(2, 0x8000)];
    input.observe_mmio = true;
    let mut cpu = retire(image.cpu, code.len());
    cpu.registers.eax = 0x1111_5678;
    let mut wanted = expected(
        &image,
        &[Step {
            cpu,
            ram: &[],
            exit: Exit::Dispatch(cpu.eip),
        }],
    );
    wanted.events.insert(
        0,
        Event::MmioRead {
            address: 0x2000,
            bytes: 2,
        },
    );
    assert_eq!(module.observe(&input, 1), wanted);
}
