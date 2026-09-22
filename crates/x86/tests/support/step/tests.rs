use super::{CallPatches, Engine, Input, TestModule};
use wasm86_compiler::{MemoryImport, Program, Signature, Type, I32};

fn check_transient_machine_write(engine: Engine) {
    let mut program = Program::new();
    let machine = program.import_memory(MemoryImport {
        module: "wasm86".into(),
        name: "machine".into(),
        minimum: 64,
        maximum: None,
        shared: false,
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
        segment_profile: None,
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

fn check_host_memory_edits(engine: Engine) {
    use super::{Argument, Event, Observation, Outcome, Snapshot};

    let mut program = Program::new();
    let entry = program
        .function(
            Signature {
                parameters: vec![],
                results: vec![Type::I32],
            },
            |body| body.return_(0),
        )
        .unwrap();
    program.export("entry", entry).unwrap();
    let module = TestModule::new(&crate::CompiledModule {
        segment_profile: None,
        bytes: program.compile().unwrap(),
        entry: "entry".into(),
    });
    let input = Input {
        observe_guest: true,
        patches_before_calls: vec![
            CallPatches {
                cpu: vec![(0, vec![3])],
                guest: vec![(7, vec![41])],
                machine: vec![(12, vec![1])],
            },
            CallPatches {
                guest: vec![(7, vec![0])],
                machine: vec![(12, vec![0])],
                ..CallPatches::default()
            },
        ],
        ..Input::new(&[0])
    };
    assert_eq!(
        engine.observe(&module, &input, 2),
        Observation {
            events: [vec![(7, 41)], vec![]]
                .into_iter()
                .map(|guest| Event::Return {
                    outcome: Outcome::Returned(vec![Argument::I32(0)]),
                    snapshot: Snapshot {
                        cpu: vec![3],
                        guest: Some(guest),
                    },
                })
                .collect(),
            guest_unchanged: true,
            machine_unchanged: false,
        }
    );
}

#[test]
fn host_edits_are_observed_without_module_memory_imports() {
    check_host_memory_edits(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_observes_host_edits_without_module_memory_imports() {
    check_host_memory_edits(Engine::V8);
}

fn check_entry_contexts(engine: Engine) {
    use super::{Argument, Event, Observation, Outcome, Snapshot};
    use crate::{compile_block_from_bytes, CpuState, SegmentProfile, Segments};

    let module = TestModule::new(&compile_block_from_bytes(0x1000, &[0x90], 1).unwrap());
    assert_eq!(module.profile, Some(SegmentProfile::Flat32));
    for profile in [SegmentProfile::Flat32, SegmentProfile::Segmented32] {
        assert_eq!(
            TestModule::interpreter_with_profile(profile).profile,
            Some(profile)
        );
    }
    let invalid = CpuState::filled(0xa5);
    let mut cpu = invalid;
    cpu.segments = Segments::flat32();
    cpu.eip = 0x1000;
    cpu.instruction_count = 4;
    let input = Input {
        patches_before_calls: vec![
            CallPatches::cpu(vec![(0, cpu.to_bytes().to_vec())]),
            CallPatches::cpu(vec![(0, invalid.to_bytes().to_vec())]),
        ],
        ..Input::new(&invalid.to_bytes())
    };
    cpu.eip = 0x1001;
    cpu.instruction_count = 5;
    let snapshot = Snapshot {
        cpu: cpu.to_bytes().to_vec(),
        guest: None,
    };
    assert_eq!(
        engine.observe(&module, &input, 1),
        Observation {
            events: vec![
                Event::Dispatch {
                    eip: 0x1001,
                    snapshot: snapshot.clone()
                },
                Event::Return {
                    outcome: Outcome::Returned(vec![Argument::I64(i64::MIN)]),
                    snapshot
                },
            ],
            guest_unchanged: true,
            machine_unchanged: true,
        }
    );
}

#[test]
fn profile_admission_uses_patched_contexts_for_actual_calls() {
    check_entry_contexts(Engine::Wasmtime);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_profile_admission_uses_actual_entry_contexts() {
    check_entry_contexts(Engine::V8);
}
