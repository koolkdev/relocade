use super::{CallPatches, Input, TestModule};
use crate::SegmentProfile;
use wasm86_compiler::{MemoryImport, Program, Signature, Type, I32};

#[test]
fn machine_changes_are_observed_before_a_later_call_restores_them() {
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
        execution_target: None,
        bytes: program.compile().unwrap(),
        entry: "toggle".into(),
    });
    let observation = module.observe(&Input::new(&[]), 2);
    // Two toggles restore the initial bytes, but the first return changed them.
    assert!(!observation.machine_unchanged);
}

#[test]
fn host_edits_are_observed_without_module_memory_imports() {
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
        execution_target: None,
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
        module.observe(&input, 2),
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
fn profile_admission_uses_patched_contexts_for_actual_calls() {
    use super::{Argument, Event, Observation, Outcome, Snapshot};
    use crate::{compile_block_from_bytes, CpuState, ExecutionProfile, Segments};

    let module = TestModule::new(&compile_block_from_bytes(0x1000, &[0x90], 1).unwrap());
    assert_eq!(
        module.target.map(|target| target.profile),
        Some(ExecutionProfile::Protected(SegmentProfile::Flat32))
    );
    for profile in [
        ExecutionProfile::Protected(SegmentProfile::Flat32),
        ExecutionProfile::Protected(SegmentProfile::Segmented32),
    ] {
        assert_eq!(
            TestModule::interpreter_with_profile(profile)
                .target
                .map(|target| target.profile),
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
        module.observe(&input, 1),
        Observation {
            events: vec![
                Event::Dispatch {
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
fn interpreter_links_require_the_same_cpu_model() {
    use crate::{Compiler, CpuFeature, CpuModel};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    let compilers = [
        Compiler::new(SegmentProfile::Flat32),
        Compiler::new(SegmentProfile::Flat32)
            .with_cpu_model(CpuModel::default().with_feature(CpuFeature::Cmov, false)),
    ];
    for block_compiler in &compilers {
        let block = block_compiler.compile_block(0x1000, &[0x90], 1).unwrap();
        for interpreter_compiler in &compilers {
            let interpreter = TestModule::interpreter_with_target(interpreter_compiler.target());
            assert_eq!(interpreter.target, Some(interpreter_compiler.target()));
            let link = catch_unwind(AssertUnwindSafe(|| {
                TestModule::new(&block).with_interpreter(interpreter)
            }));
            assert_eq!(
                link.is_ok(),
                block_compiler.target() == interpreter_compiler.target()
            );
        }
    }
}
