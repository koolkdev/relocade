use super::{Engine, Input, TestModule};
use wasm86_compiler::{MemoryImport, Program, Signature, Type, I32};

fn check_transient_machine_write(engine: Engine) {
    let mut program = Program::new();
    let machine = program.import_memory(MemoryImport {
        module: "wasm86".into(),
        name: "machine".into(),
        minimum: 64,
        maximum: None,
    });
    let toggle = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I32],
            },
            |mut body| {
                let value = body.load::<I32>(machine, 0)?;
                body.store(machine, 0, value.xor(1))?;
                body.return_(0)
            },
        )
        .unwrap();
    program.export("toggle", toggle).unwrap();
    let module = TestModule::new(&crate::CompiledModule {
        bytes: program.compile().unwrap(),
        entry: "toggle".into(),
    });
    let observation = engine.observe(&module, &Input::new(&[]), 2);
    // Two toggles restore the initial bytes, but the first return changed them.
    assert!(!observation.machine_unchanged);
}

#[test]
fn machine_changes_are_observed_before_a_later_call_restores_them() {
    check_transient_machine_write(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_observes_machine_changes_before_a_later_call_restores_them() {
    check_transient_machine_write(Engine::V8);
}
