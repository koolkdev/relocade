//! A handoff interpreter supplies the imports required by its own memory model.

use super::*;
use crate::support::{
    machine::expected,
    step::{Event, TestModule},
};
use wasm86_compiler::{FunctionImport, Program, Signature, Type};

fn interpreter_imports(engine: Engine) {
    // The caller only imports interpret; its Real16 callee requires physical memory.
    let mut program = Program::new();
    let signature = Signature {
        parameters: vec![],
        results: vec![Type::I64],
    };
    let interpret = program.import_function(FunctionImport {
        module: "wasm86".into(),
        name: "interpret".into(),
        signature: signature.clone(),
    });
    let entry = program
        .function(signature, |body| body.tail_call(interpret, &[]))
        .unwrap();
    program.export("enter", entry).unwrap();
    let module = TestModule::new(&wasm86_x86::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "enter".into(),
        execution_profile: Some(ExecutionProfile::Real16),
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
    let mut cpu = retired(&image, code.len());
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
    assert_eq!(engine.observe(&module, &input, 1), wanted);
}

#[test]
fn handoff_registers_the_real_mode_interpreters_memory_imports() {
    interpreter_imports(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_handoff_registers_the_real_mode_interpreters_memory_imports() {
    interpreter_imports(Engine::V8);
}
