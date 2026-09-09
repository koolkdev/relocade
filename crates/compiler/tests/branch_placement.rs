#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, FunctionImport, Mem, MemoryImport, Program, Signature, Type,
    I1, I32,
};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

fn module(
    parameters: &[Type],
    build: impl FnOnce(FunctionBuilder<'_>, Mem, Func) -> Result<(), BuildError>,
) -> Vec<u8> {
    let mut program = Program::new();
    let state = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    });
    let receive = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: Signature {
            parameters: vec![Type::I32],
            result: Type::I32,
        },
    });
    let run = program
        .function(
            Signature {
                parameters: parameters.to_vec(),
                result: Type::I32,
            },
            |body| build(body, state, receive),
        )
        .unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn exclusive_switch_arms() -> Vec<u8> {
    module(&[Type::I32, Type::I32, Type::I32], |mut body, state, _| {
        let selector = body.parameter::<I32>(0)?;
        let input = body.parameter::<I32>(1)?;
        let address = body.parameter::<I32>(2)?;
        let shared = input.xor(0x8000_0000u32);
        let result = body.switch_value::<I32, _>(selector, &[10, 11, 12, 13], |mut arm, key| {
            if let Some(key) = key {
                arm.store_at(state, &address, 0, &shared)?;
                arm.store::<I32>(state, 4, key)?;
                arm.yield_(&shared)
            } else {
                arm.yield_(17)
            }
        })?;
        body.return_(result)
    })
}

fn nested_uses_in_switch_arms() -> Vec<u8> {
    module(
        &[Type::I32, Type::I1, Type::I1, Type::I32, Type::I32],
        |mut body, state, _| {
            let selector = body.parameter::<I32>(0)?;
            let first = body.parameter::<I1>(1)?;
            let second = body.parameter::<I1>(2)?;
            let shared = body.parameter::<I32>(3)?.xor(1);
            let address = body.parameter::<I32>(4)?;
            let result =
                body.switch_value::<I32, _>(selector, &[10, 11, 12, 13], |mut arm, key| {
                    if let Some(key) = key {
                        arm.if_(&first, |mut inner| {
                            inner.store_at(state, &address, 0, &shared)
                        })?;
                        arm.if_(&second, |mut inner| {
                            inner.store_at(state, &address, 4, &shared)
                        })?;
                        arm.store::<I32>(state, 8, key)?;
                        arm.yield_(&shared)
                    } else {
                        arm.yield_(17)
                    }
                })?;
            body.return_(result)
        },
    )
}

fn dependency_used_after_the_join() -> Vec<u8> {
    module(&[Type::I1, Type::I32], |mut body, state, _| {
        let condition = body.parameter::<I1>(0)?;
        let base = body.parameter::<I32>(1)?.add(1);
        let branch_value = base.xor(7);
        body.if_else(
            condition,
            |mut arm| arm.store(state, 0, &branch_value),
            |mut arm| arm.store(state, 4, &branch_value),
        )?;
        body.return_(base)
    })
}

fn sequential_controls() -> Vec<u8> {
    module(&[Type::I1, Type::I1, Type::I32], |mut body, state, _| {
        let first = body.parameter::<I1>(0)?;
        let second = body.parameter::<I1>(1)?;
        let shared = body.parameter::<I32>(2)?.xor(1);
        body.if_(first, |mut arm| arm.store(state, 0, &shared))?;
        body.if_(second, |mut arm| arm.store(state, 4, &shared))?;
        body.return_(17)
    })
}

fn sequential_controls_inside_an_exclusive_arm() -> Vec<u8> {
    module(
        &[Type::I1, Type::I1, Type::I1, Type::I32],
        |mut body, state, _| {
            let outer = body.parameter::<I1>(0)?;
            let first = body.parameter::<I1>(1)?;
            let second = body.parameter::<I1>(2)?;
            let shared = body.parameter::<I32>(3)?.xor(1);
            body.if_else(
                outer,
                |mut arm| {
                    arm.if_(first, |mut inner| inner.store(state, 0, &shared))?;
                    arm.if_(second, |mut inner| inner.store(state, 4, &shared))
                },
                |mut arm| arm.store(state, 8, &shared),
            )?;
            body.return_(17)
        },
    )
}

fn shared_address_needed_by_a_parent_load() -> Vec<u8> {
    module(&[Type::I1, Type::I32], |mut body, state, _| {
        let condition = body.parameter::<I1>(0)?;
        let address = body.parameter::<I32>(1)?.add(4);
        let previous = body.load_at::<I32>(state, &address, 0)?;
        body.if_else(
            condition,
            |mut arm| {
                arm.store_at::<I32>(state, &address, 0, 13)?;
                arm.store(state, 0, &previous)
            },
            |mut arm| {
                arm.store_at::<I32>(state, &address, 0, 15)?;
                arm.store(state, 0, &previous)
            },
        )?;
        body.return_(17)
    })
}

#[derive(Clone, Copy)]
enum Snapshot {
    Load,
    Call,
    Join,
}

fn snapshot_across_arm_writes(source: Snapshot) -> Vec<u8> {
    module(&[Type::I1, Type::I1], |mut body, state, receive| {
        let destination = body.parameter::<I1>(0)?;
        let choose_load = body.parameter::<I1>(1)?;
        let previous = match source {
            Snapshot::Load => body.load::<I32>(state, 0)?,
            Snapshot::Call => body.call::<I32>(receive, &[9.into()])?,
            Snapshot::Join => body.if_value::<I32>(
                choose_load,
                |mut arm| {
                    let previous = arm.load::<I32>(state, 0)?;
                    arm.yield_(previous)
                },
                |arm| arm.yield_(11),
            )?,
        };
        let derived = previous.xor(1);
        body.if_else(
            destination,
            |mut arm| {
                arm.store::<I32>(state, 0, 13)?;
                arm.store(state, 4, &derived)
            },
            |mut arm| {
                arm.store::<I32>(state, 0, 15)?;
                arm.store(state, 4, &derived)
            },
        )?;
        body.return_(17)
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Event {
    If,
    Else,
    End,
    Table,
    BlockEnd,
    Add,
    Xor,
    Load,
    Store,
    Call,
}

fn inspect(bytes: &[u8]) -> Vec<Event> {
    Validator::new().validate_all(bytes).unwrap();
    let mut imports = 0;
    let mut run = None;
    let mut body_index = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(section) => {
                for import in section {
                    if matches!(import.unwrap().ty, TypeRef::Func(_)) {
                        imports += 1;
                    }
                }
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.name == "run" {
                        run = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                if Some(imports + body_index) != run {
                    body_index += 1;
                    continue;
                }
                let mut events = Vec::new();
                let mut controls = Vec::new();
                for operator in body.get_operators_reader().unwrap() {
                    let event = match operator.unwrap() {
                        Operator::If { .. } => {
                            controls.push(true);
                            Event::If
                        }
                        Operator::Block { .. } => {
                            controls.push(false);
                            continue;
                        }
                        Operator::Else => Event::Else,
                        Operator::End => match controls.pop() {
                            Some(true) => Event::End,
                            Some(false) => Event::BlockEnd,
                            None => continue,
                        },
                        Operator::BrTable { .. } => Event::Table,
                        Operator::I32Add => Event::Add,
                        Operator::I32Xor => Event::Xor,
                        Operator::I32Load { .. } => Event::Load,
                        Operator::I32Store { .. } => Event::Store,
                        Operator::Call { .. } => Event::Call,
                        _ => continue,
                    };
                    events.push(event);
                }
                return events;
            }
            _ => {}
        }
    }
    panic!("the run export has no body");
}

// These fixtures contain only fallthrough If/Else controls. Count arithmetic on
// possible paths so two sequential inner arms cannot masquerade as exclusivity.
fn xor_counts_on_paths(events: &[Event], cursor: &mut usize) -> Vec<usize> {
    let mut counts = vec![0];
    while *cursor < events.len() {
        let event = events[*cursor];
        *cursor += 1;
        match event {
            Event::If => {
                let mut arms = xor_counts_on_paths(events, cursor);
                if events[*cursor - 1] == Event::Else {
                    arms.extend(xor_counts_on_paths(events, cursor));
                } else {
                    arms.push(0);
                }
                counts = counts
                    .iter()
                    .flat_map(|prefix| arms.iter().map(move |arm| prefix + arm))
                    .collect();
            }
            Event::Else | Event::End => return counts,
            Event::Xor => counts.iter_mut().for_each(|count| *count += 1),
            Event::Table => panic!("path counter is only for the If/Else fixtures"),
            _ => {}
        }
    }
    counts
}

#[test]
fn pure_values_shared_within_exclusive_cases_stay_after_dispatch() {
    let events = inspect(&exclusive_switch_arms());
    let dispatch = events
        .iter()
        .position(|event| *event == Event::Table)
        .unwrap();
    assert!(!events[..dispatch].contains(&Event::Xor));
    assert_eq!(
        events.iter().filter(|event| **event == Event::Xor).count(),
        4
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| **event == Event::Store)
            .count(),
        8
    );
}

#[test]
fn nested_uses_share_one_capture_inside_each_selected_switch_arm() {
    let events = inspect(&nested_uses_in_switch_arms());
    let dispatch = events
        .iter()
        .position(|event| *event == Event::Table)
        .unwrap();
    assert!(!events[..dispatch].contains(&Event::Xor));
    // Each switch label closes its dispatch block before its case body. The
    // nested controls here are If/Else, so their ends do not divide the cases.
    let arms: Vec<_> = events[dispatch + 1..]
        .split(|event| *event == Event::BlockEnd)
        .filter(|arm| arm.contains(&Event::If))
        .collect();
    assert_eq!(arms.len(), 4);
    for arm in arms {
        assert_eq!(xor_counts_on_paths(arm, &mut 0), [1, 1, 1, 1], "{arm:?}");
    }
}

#[test]
fn a_shared_dependency_needed_after_the_join_keeps_its_capture() {
    let events = inspect(&dependency_used_after_the_join());
    let branch = events.iter().position(|event| *event == Event::If).unwrap();
    assert_eq!(
        events.iter().filter(|event| **event == Event::Add).count(),
        1
    );
    assert!(events[..branch].contains(&Event::Add));
    assert!(!events[..branch].contains(&Event::Xor));
    assert_eq!(xor_counts_on_paths(&events, &mut 0), [1, 1]);
}

#[test]
fn sequential_controls_do_not_recompute_a_shared_value_on_the_same_path() {
    for bytes in [
        sequential_controls(),
        sequential_controls_inside_an_exclusive_arm(),
    ] {
        let events = inspect(&bytes);
        let counts = xor_counts_on_paths(&events, &mut 0);
        assert_eq!(counts.iter().max(), Some(&1), "{events:?}");
    }
}

#[test]
fn nested_sequential_uses_stay_inside_their_outer_conditional_arm() {
    let events = inspect(&sequential_controls_inside_an_exclusive_arm());
    let branch = events.iter().position(|event| *event == Event::If).unwrap();
    assert!(!events[..branch].contains(&Event::Xor));
    assert_eq!(xor_counts_on_paths(&events, &mut 0).iter().max(), Some(&1));
}

#[test]
fn a_shared_address_is_available_when_the_parent_captures_its_load() {
    let events = inspect(&shared_address_needed_by_a_parent_load());
    assert_eq!(
        events.iter().filter(|event| **event == Event::Add).count(),
        1
    );
    let address = events
        .iter()
        .position(|event| *event == Event::Add)
        .unwrap();
    let read = events
        .iter()
        .position(|event| *event == Event::Load)
        .unwrap();
    let branch = events.iter().position(|event| *event == Event::If).unwrap();
    assert!(address < read && read < branch, "{events:?}");
}

#[test]
fn load_call_and_join_inputs_keep_their_snapshot_before_arm_writes() {
    for source in [Snapshot::Load, Snapshot::Call, Snapshot::Join] {
        let events = inspect(&snapshot_across_arm_writes(source));
        let input = match source {
            Snapshot::Call => Event::Call,
            _ => Event::Load,
        };
        assert_eq!(events.iter().filter(|event| **event == input).count(), 1);
        let read = events.iter().position(|event| *event == input).unwrap();
        let write = events
            .iter()
            .position(|event| *event == Event::Store)
            .unwrap();
        assert!(read < write, "{events:?}");
        assert_eq!(xor_counts_on_paths(&events, &mut 0).iter().max(), Some(&1));
    }
}

fn check_execution(flags: &[&str]) {
    let exclusive = ModuleFile::new(&exclusive_switch_arms());
    for (selector, state) in [
        ("i32:10", "070000800a00000006000000"),
        ("i32:11", "070000800b00000006000000"),
        ("i32:12", "070000800c00000006000000"),
        ("i32:13", "070000800d00000006000000"),
    ] {
        exclusive.check(
            flags,
            "execute-tail.mjs",
            &[
                "run",
                "070000000500000006000000",
                "",
                selector,
                "i32:7",
                "i32:0",
            ],
            &format!("return -2147483641\nstate {state}\n"),
        );
    }
    exclusive.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "070000000500000006000000",
            "",
            "i32:10",
            "i32:-2147483648",
            "i32:0",
        ],
        "return 0\nstate 000000000a00000006000000\n",
    );
    exclusive.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "070000000500000006000000",
            "",
            "i32:9",
            "i32:7",
            "i32:65536",
        ],
        "return 17\nstate 070000000500000006000000\n",
    );
    exclusive.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "070000000500000006000000",
            "",
            "i32:10",
            "i32:7",
            "i32:65536",
        ],
        "return trap\nstate 070000000500000006000000\n",
    );

    let nested_switch = ModuleFile::new(&nested_uses_in_switch_arms());
    for (selector, first, second, state) in [
        ("i32:10", "i32:1", "i32:1", "08000000080000000a000000"),
        ("i32:11", "i32:1", "i32:0", "08000000050000000b000000"),
        ("i32:12", "i32:0", "i32:1", "07000000080000000c000000"),
        ("i32:13", "i32:0", "i32:0", "07000000050000000d000000"),
    ] {
        nested_switch.check(
            flags,
            "execute-tail.mjs",
            &[
                "run",
                "070000000500000006000000",
                "",
                selector,
                first,
                second,
                "i32:9",
                "i32:0",
            ],
            &format!("return 8\nstate {state}\n"),
        );
    }
    for (selector, first, second, expected) in [
        (
            "i32:9",
            "i32:1",
            "i32:1",
            "return 17\nstate 070000000500000006000000\n",
        ),
        (
            "i32:10",
            "i32:0",
            "i32:0",
            "return 8\nstate 07000000050000000a000000\n",
        ),
        (
            "i32:10",
            "i32:1",
            "i32:0",
            "return trap\nstate 070000000500000006000000\n",
        ),
    ] {
        nested_switch.check(
            flags,
            "execute-tail.mjs",
            &[
                "run",
                "070000000500000006000000",
                "",
                selector,
                first,
                second,
                "i32:9",
                "i32:65536",
            ],
            expected,
        );
    }

    let dependency = ModuleFile::new(&dependency_used_after_the_join());
    for (condition, state) in [
        ("i32:1", "0d0000000500000006000000"),
        ("i32:0", "070000000d00000006000000"),
    ] {
        dependency.check(
            flags,
            "execute-tail.mjs",
            &["run", "070000000500000006000000", "", condition, "i32:9"],
            &format!("return 10\nstate {state}\n"),
        );
    }
    let sequential = ModuleFile::new(&sequential_controls());
    for (first, second, state) in [
        ("i32:1", "i32:1", "060000000600000006000000"),
        ("i32:0", "i32:1", "070000000600000006000000"),
        ("i32:0", "i32:0", "070000000500000006000000"),
    ] {
        sequential.check(
            flags,
            "execute-tail.mjs",
            &[
                "run",
                "070000000500000006000000",
                "",
                first,
                second,
                "i32:7",
            ],
            &format!("return 17\nstate {state}\n"),
        );
    }
    let nested = ModuleFile::new(&sequential_controls_inside_an_exclusive_arm());
    for (outer, first, second, state) in [
        ("i32:1", "i32:1", "i32:1", "080000000800000006000000"),
        ("i32:1", "i32:1", "i32:0", "080000000500000006000000"),
        ("i32:0", "i32:1", "i32:1", "070000000500000008000000"),
    ] {
        nested.check(
            flags,
            "execute-tail.mjs",
            &[
                "run",
                "070000000500000006000000",
                "",
                outer,
                first,
                second,
                "i32:9",
            ],
            &format!("return 17\nstate {state}\n"),
        );
    }
    let address = ModuleFile::new(&shared_address_needed_by_a_parent_load());
    for (condition, input, expected) in [
        (
            "i32:1",
            "i32:0",
            "return 17\nstate 050000000d00000006000000\n",
        ),
        (
            "i32:0",
            "i32:0",
            "return 17\nstate 050000000f00000006000000\n",
        ),
        (
            "i32:1",
            "i32:4",
            "return 17\nstate 06000000050000000d000000\n",
        ),
        (
            "i32:0",
            "i32:65532",
            "return trap\nstate 070000000500000006000000\n",
        ),
    ] {
        address.check(
            flags,
            "execute-tail.mjs",
            &["run", "070000000500000006000000", "", condition, input],
            expected,
        );
    }

    for source in [Snapshot::Load, Snapshot::Call, Snapshot::Join] {
        let snapshot = ModuleFile::new(&snapshot_across_arm_writes(source));
        for (destination, state) in [
            ("i32:1", "0d0000000600000006000000"),
            ("i32:0", "0f0000000600000006000000"),
        ] {
            let callback = if matches!(source, Snapshot::Call) {
                "receive(9) 070000000500000006000000\n"
            } else {
                ""
            };
            snapshot.check(
                flags,
                "execute-tail.mjs",
                &[
                    "run",
                    "070000000500000006000000",
                    "receive:i32:7",
                    destination,
                    "i32:1",
                ],
                &format!("{callback}return 17\nstate {state}\n"),
            );
        }
    }
    ModuleFile::new(&snapshot_across_arm_writes(Snapshot::Join)).check(
        flags,
        "execute-tail.mjs",
        &["run", "070000000500000006000000", "", "i32:1", "i32:0"],
        "return 17\nstate 0d0000000a00000006000000\n",
    );
}

#[test]
#[ignore = "requires Node.js with WebAssembly support"]
fn branch_placement_executes_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js with the optimizing WebAssembly flags"]
fn branch_placement_executes_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
