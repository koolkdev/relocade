//! Atomic effect placement around reads, helper calls and control flow.

use wasm86_compiler::{MemoryImport, Program, Signature, Type, I32};
use wasmparser::{Operator, Parser, Payload};

use crate::{
    fixture::Fixture,
    wasm::{Input, MemoryBytes, Observation, Value},
};

#[test]
fn shared_imports_do_not_change_ordinary_memory_lowering() {
    let compile = |shared| {
        let mut program = Program::new();
        let memory = program.import_memory(MemoryImport {
            module: "test".into(),
            name: "memory".into(),
            minimum: 1,
            maximum: Some(1),
            shared,
        });
        let run = program
            .function(
                Signature {
                    parameters: vec![],
                    results: vec![Type::I32],
                },
                |mut body| {
                    let value = body.load::<I32>(memory, 0)?;
                    body.store::<I32>(memory, 4, 7)?;
                    body.return_(value)
                },
            )
            .unwrap();
        program.export("run", run).unwrap();
        program.compile().unwrap()
    };
    let code = |bytes: &[u8]| {
        Parser::new(0)
            .parse_all(bytes)
            .filter_map(|payload| match payload.unwrap() {
                Payload::CodeSectionEntry(body) => Some(body.as_bytes().to_vec()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(code(&compile(false)), code(&compile(true)));
}

#[derive(Clone, Copy, Debug)]
enum Synchronization {
    Fence,
    Atomic,
    Helper,
}

#[test]
fn synchronization_orders_direct_reads_and_read_only_helpers_across_memories() {
    for synchronization in [
        Synchronization::Fence,
        Synchronization::Atomic,
        Synchronization::Helper,
    ] {
        for helper_reads in [false, true] {
            let mut fixture = Fixture::new();
            let data = fixture.memory("data", &[7; 4]);
            let control = fixture.shared_memory("control", &[0; 4]);
            let signature = Signature {
                parameters: vec![],
                results: vec![Type::I32],
            };
            let reader = fixture
                .program
                .function(signature.clone(), |mut body| {
                    let value = body.load::<I32>(data, 0)?;
                    body.return_(value)
                })
                .unwrap();
            let inner = fixture
                .program
                .function(signature.clone(), |mut body| {
                    body.atomic::<I32>(control, 0, 0)?.load()?;
                    body.return_(0)
                })
                .unwrap();
            let outer = fixture
                .program
                .function(signature.clone(), |mut body| {
                    body.call::<I32>(inner, &[])?;
                    body.return_(0)
                })
                .unwrap();
            let run = fixture
                .program
                .function(signature, |mut body| {
                    let before = if helper_reads {
                        body.call::<I32>(reader, &[])?
                    } else {
                        body.load::<I32>(data, 0)?
                    };
                    match synchronization {
                        Synchronization::Fence => body.atomic_fence(),
                        Synchronization::Atomic => {
                            body.atomic::<I32>(control, 0, 0)?.load()?;
                        }
                        Synchronization::Helper => {
                            body.call::<I32>(outer, &[])?;
                        }
                    }
                    let after = if helper_reads {
                        body.call::<I32>(reader, &[])?
                    } else {
                        body.load::<I32>(data, 0)?
                    };
                    body.return_(before.add(after))
                })
                .unwrap();
            let module = fixture.finish(run);
            let mut events = Vec::new();
            for payload in Parser::new(0).parse_all(module.bytes()) {
                if let Payload::CodeSectionEntry(body) = payload.unwrap() {
                    events.clear();
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::I32Load { memarg } if memarg.memory == 0 => {
                                events.push("read")
                            }
                            // The reader is the first defined function; there are no function imports.
                            Operator::Call { function_index: 0 } => events.push("read"),
                            Operator::AtomicFence
                            | Operator::I32AtomicLoad { .. }
                            | Operator::Call { .. } => events.push("synchronize"),
                            _ => {}
                        }
                    }
                }
            }
            assert_eq!(
                events,
                ["read", "synchronize", "read"],
                "{synchronization:?}, helper reads: {helper_reads}"
            );
        }
    }
}

fn check_loop(v8: bool) {
    let mut fixture = Fixture::new();
    let memory = fixture.shared_memory("counter", &10u32.to_le_bytes());
    let module = fixture.function(&[Type::I32], &[Type::I32], |mut body| {
        let count = body.parameter::<I32>(0)?;
        let sum =
            body.loop_::<(I32, I32), I32>((0, 0), |mut iteration, labels, (index, sum)| {
                iteration.if_(index.eq(&count), |done| done.branch(&labels.exit, &sum))?;
                let contribution = iteration.if_value::<I32>(
                    index.and(1).eq(0),
                    |mut even| {
                        let previous = even.atomic::<I32>(memory, 0, 0)?.add(1)?;
                        even.yield_(previous.add(&previous))
                    },
                    |odd| odd.yield_(0),
                )?;
                iteration.branch(&labels.again, (index.add(1), sum.add(contribution)))
            })?;
        body.return_(sum)
    });
    for (count, sum, counter) in [(0, 0, 10u32), (1, 20, 11), (5, 66, 13)] {
        if v8 {
            assert_eq!(
                module.run_v8(
                    &Input::call("run", &[Value::I32(count)])
                        .with_memories(&[MemoryBytes::new("counter", &10u32.to_le_bytes())])
                ),
                Observation::returned(&[Value::I32(sum)])
                    .with_memories(&[MemoryBytes::new("counter", &counter.to_le_bytes())])
            );
        } else {
            let mut instance = module.instantiate();
            assert_eq!(instance.call::<i32>(count).unwrap(), sum);
            assert_eq!(&instance.memory("counter")[..4], counter.to_le_bytes());
        }
    }
}

#[test]
fn atomic_results_execute_once_per_taken_loop_path() {
    check_loop(false);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_atomic_results_execute_once_per_taken_loop_path() {
    check_loop(true);
}
