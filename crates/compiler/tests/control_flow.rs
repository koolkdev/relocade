#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{
    BuildError, FunctionBuilder, FunctionImport, IntType, Mem, MemoryImport, Program, Signature,
    Type, Val, I1, I32, I8,
};
use wasmparser::{Operator, Parser, Payload, Validator};

fn memory(program: &mut Program) -> Mem {
    program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    })
}

fn module<T: IntType>(
    parameters: &[Type],
    build: impl FnOnce(&mut FunctionBuilder<'_>, Mem) -> Val<T>,
) -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let function = program.declare(Signature {
        parameters: parameters.to_vec(),
        result: T::TYPE,
    });
    let mut body = program.define(function).unwrap();
    let result = build(&mut body, state);
    body.return_(&result).unwrap();
    program.export("run", function).unwrap();
    program.compile().unwrap()
}

fn stores_and_tail() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let callback = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: Signature {
            parameters: vec![Type::I32],
            result: Type::I64,
        },
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I1],
        result: Type::I64,
    });
    let mut body = program.define(function).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    body.store::<I32>(state, 0, 1).unwrap();
    body.if_(&condition, |branch| {
        branch.return_(0x8000_0000_0000_0000u64)
    })
    .unwrap();
    body.store::<I32>(state, 4, 2).unwrap();
    body.tail_call(callback, &[3.into()]).unwrap();
    program.export("run", function).unwrap();
    program.compile().unwrap()
}

fn alternative_stores() -> Vec<u8> {
    module(&[Type::I1, Type::I32], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let address = body.parameter::<I32>(1).unwrap();
        let previous = body.load::<I32>(state, 0).unwrap();
        body.if_else(
            condition,
            |mut arm| {
                let value = arm.load_at::<I32>(state, &address, 0)?;
                arm.store(state, 0, value.add(2))
            },
            |mut arm| arm.store::<I32>(state, 4, 11),
        )
        .unwrap();
        previous.add(body.load::<I32>(state, 0).unwrap())
    })
}

#[test]
fn alternative_stores_preserve_the_prior_snapshot_and_selected_effects() {
    assert_eq!(
        inspect(&alternative_stores()).events,
        [
            Event::Load(0),
            Event::If,
            Event::Load(0),
            Event::Store(0),
            Event::Else,
            Event::Store(4),
            Event::End,
            Event::Load(0),
            Event::Return,
        ]
    );
}

fn continuation_load(crosses_store: bool) -> Vec<u8> {
    module(&[Type::I1, Type::I32], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let address = body.parameter::<I32>(1).unwrap();
        let loaded = body.load_at::<I32>(state, &address, 0).unwrap();
        body.if_(&condition, |branch| branch.return_(17)).unwrap();
        if crosses_store {
            body.store_at::<I32>(state, &address, 0, 9).unwrap();
        }
        loaded
    })
}

fn conditional_result_load() -> Vec<u8> {
    module(&[Type::I1, Type::I32], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let address = body.parameter::<I32>(1).unwrap();
        let loaded = body.load_at::<I32>(state, &address, 0).unwrap();
        body.if_(&condition, |branch| branch.return_(&loaded))
            .unwrap();
        body.store::<I32>(state, 0, 9).unwrap();
        body.value::<I32>(17).unwrap()
    })
}

fn shared_snapshot() -> Vec<u8> {
    module(&[], |body, state| {
        let loaded = body.load::<I32>(state, 0).unwrap();
        let shared = loaded.add(1);
        body.if_(loaded.eq(7), |branch| branch.return_(&shared))
            .unwrap();
        body.store::<I32>(state, 0, 9).unwrap();
        shared
    })
}

fn sequential_exits() -> Vec<u8> {
    module(&[Type::I1, Type::I32], |body, state| {
        let first = body.parameter::<I1>(0).unwrap();
        body.if_(&first, |branch| branch.return_(11)).unwrap();
        let address = body.parameter::<I32>(1).unwrap();
        let loaded = body.load_at::<I32>(state, &address, 0).unwrap();
        body.if_(loaded.eq(7), |branch| branch.return_(22)).unwrap();
        body.store::<I32>(state, 4, 9).unwrap();
        body.value::<I32>(33).unwrap()
    })
}

fn constant_guard(taken: bool) -> Vec<u8> {
    module(&[], |body, state| {
        body.store::<I32>(state, 0, 1).unwrap();
        body.if_(taken, |branch| branch.return_(17)).unwrap();
        body.store::<I32>(state, 4, 2).unwrap();
        body.value::<I32>(33).unwrap()
    })
}

fn narrow_condition_and_results() -> Vec<u8> {
    module(&[Type::I1, Type::I8], |body, _state| {
        let condition = body.parameter::<I1>(0).unwrap().add(1);
        let value = body.parameter::<I8>(1).unwrap().add(1);
        body.if_(&condition, |branch| branch.return_(&value))
            .unwrap();
        value.add(1)
    })
}

fn branch_load_after_store() -> Vec<u8> {
    module(&[Type::I1, Type::I32], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let address = body.parameter::<I32>(1).unwrap();
        body.store::<I32>(state, 0, 1).unwrap();
        body.if_(&condition, |mut branch| {
            branch.store::<I32>(state, 4, 2)?;
            let loaded = branch.load_at::<I32>(state, &address, 0)?;
            branch.return_(&loaded)
        })
        .unwrap();
        body.store::<I32>(state, 8, 3).unwrap();
        body.value::<I32>(17).unwrap()
    })
}

fn fallthrough_snapshot() -> Vec<u8> {
    module(&[Type::I1], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let before = body.load_at::<I32>(state, 0, 0).unwrap();
        body.if_(&condition, |mut branch| {
            branch.store_at::<I32>(state, 0, 0, 9)?;
            Ok(())
        })
        .unwrap();
        let after = body.load_at::<I32>(state, 0, 0).unwrap();
        before.add(&after)
    })
}

fn shared_pure_value() -> Vec<u8> {
    module(&[Type::I1, Type::I32], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let input = body.parameter::<I32>(1).unwrap();
        let mut pure = None;
        body.if_(&condition, |mut branch| {
            let value = input.add(1);
            branch.store(state, 0, &value)?;
            pure = Some(value);
            Ok(())
        })
        .unwrap();
        pure.unwrap()
    })
}

fn nested_tail() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let callback = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: Signature {
            parameters: vec![Type::I32],
            result: Type::I64,
        },
    });
    let function = program.declare(Signature {
        parameters: vec![Type::I1; 2],
        result: Type::I64,
    });
    let mut body = program.define(function).unwrap();
    let outer = body.parameter::<I1>(0).unwrap();
    let inner = body.parameter::<I1>(1).unwrap();
    body.if_(&outer, |mut branch| {
        branch.store::<I32>(state, 0, 1)?;
        branch.if_(&inner, |inner| inner.tail_call(callback, &[11.into()]))?;
        branch.store::<I32>(state, 4, 2)?;
        Ok(())
    })
    .unwrap();
    body.store::<I32>(state, 8, 3).unwrap();
    body.return_(17).unwrap();
    program.export("run", function).unwrap();
    program.compile().unwrap()
}

#[derive(Debug, PartialEq)]
enum Event {
    Else,
    If,
    End,
    Load(u64),
    Store(u64),
    Return,
    Tail,
}

#[derive(Default)]
struct Code {
    events: Vec<Event>,
    additions: usize,
    masks: usize,
    writes: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            let mut depth = 0;
            let mut operators = body.get_operators_reader().unwrap();
            while !operators.eof() {
                let event = match operators.read().unwrap() {
                    Operator::If { .. } => {
                        depth += 1;
                        Event::If
                    }
                    Operator::End if depth > 0 => {
                        depth -= 1;
                        Event::End
                    }
                    Operator::Else => Event::Else,
                    Operator::I32Load { memarg } => Event::Load(memarg.offset),
                    Operator::I32Store { memarg } => Event::Store(memarg.offset),
                    Operator::Return => Event::Return,
                    Operator::ReturnCall { .. } => Event::Tail,
                    Operator::I32Add | Operator::I64Add => {
                        code.additions += 1;
                        continue;
                    }
                    Operator::I32And => {
                        code.masks += 1;
                        continue;
                    }
                    Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                        code.writes += 1;
                        continue;
                    }
                    Operator::Call { .. } => panic!("the continuation must use a tail call"),
                    _ => continue,
                };
                code.events.push(event);
            }
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn conditional_return_skips_later_stores_and_the_tail_call() {
    assert_eq!(
        inspect(&stores_and_tail()).events,
        [
            Event::Store(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(4),
            Event::Tail
        ]
    );
}

#[test]
fn loads_used_on_only_one_path_stay_on_that_path() {
    let continuing = inspect(&continuation_load(false));
    assert_eq!(
        continuing.events,
        [
            Event::If,
            Event::Return,
            Event::End,
            Event::Load(0),
            Event::Return
        ]
    );
    assert_eq!(continuing.writes, 0);
    let exiting = inspect(&conditional_result_load());
    assert_eq!(
        exiting.events,
        [
            Event::If,
            Event::Load(0),
            Event::Return,
            Event::End,
            Event::Store(0),
            Event::Return
        ]
    );
    assert_eq!(exiting.writes, 0);
}

#[test]
fn overlapping_stores_preserve_a_snapshot_across_the_guard() {
    let captured = inspect(&continuation_load(true));
    assert_eq!(
        captured.events,
        [
            Event::Load(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(0),
            Event::Return
        ]
    );
    assert_eq!(captured.writes, 1);
    let shared = inspect(&shared_snapshot());
    assert_eq!(
        shared.events,
        [
            Event::Load(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(0),
            Event::Return
        ]
    );
    assert_eq!(shared.additions, 1);
}

#[test]
fn later_exit_conditions_are_evaluated_only_after_earlier_guards() {
    assert_eq!(
        inspect(&sequential_exits()).events,
        [
            Event::If,
            Event::Return,
            Event::End,
            Event::Load(0),
            Event::If,
            Event::Return,
            Event::End,
            Event::Store(4),
            Event::Return
        ]
    );
}

#[test]
fn narrow_conditions_and_each_return_observe_their_logical_bits() {
    let code = inspect(&narrow_condition_and_results());
    assert_eq!(
        code.events,
        [Event::If, Event::Return, Event::End, Event::Return]
    );
    assert_eq!(code.masks, 3);
}

#[test]
fn branch_loads_and_stores_remain_inside_the_selected_arm() {
    assert_eq!(
        inspect(&branch_load_after_store()).events,
        [
            Event::Store(0),
            Event::If,
            Event::Store(4),
            Event::Load(0),
            Event::Return,
            Event::End,
            Event::Store(8),
            Event::Return
        ]
    );
    assert_eq!(
        inspect(&nested_tail()).events,
        [
            Event::If,
            Event::Store(0),
            Event::If,
            Event::Tail,
            Event::End,
            Event::Store(4),
            Event::End,
            Event::Store(8),
            Event::Return
        ]
    );
}

#[test]
fn falling_through_a_branch_preserves_snapshots_and_shared_values() {
    let snapshot = inspect(&fallthrough_snapshot());
    assert_eq!(
        snapshot.events,
        [
            Event::Load(0),
            Event::If,
            Event::Store(0),
            Event::End,
            Event::Load(0),
            Event::Return
        ]
    );
    assert_eq!(snapshot.writes, 1);
    let shared = inspect(&shared_pure_value());
    assert_eq!(
        shared.events,
        [Event::If, Event::Store(0), Event::End, Event::Return]
    );
    assert_eq!((shared.additions, shared.writes), (1, 1));
}

#[test]
fn child_load_dependencies_are_not_visible_to_parent_or_sibling_consumers() {
    let bytes = module(&[Type::I1], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let mut escaped = None;
        body.if_(&condition, |mut branch| {
            let loaded = branch.load::<I32>(state, 0)?;
            branch.store(state, 12, &loaded)?;
            escaped = Some(loaded);
            Ok(())
        })
        .unwrap();
        let loaded = escaped.unwrap();
        assert!(matches!(
            body.value(loaded.add(1)),
            Err(BuildError::OutOfScope)
        ));
        body.if_(&condition, |mut sibling| {
            assert_eq!(
                sibling.store(state, 8, &loaded),
                Err(BuildError::OutOfScope)
            );
            Ok(())
        })
        .unwrap();
        body.value::<I32>(7).unwrap()
    });
    Validator::new().validate_all(&bytes).unwrap();
}

fn check_execution(flags: &[&str]) {
    let alternatives = ModuleFile::new(&alternative_stores());
    for (condition, address, expected) in [
        ("i32:1", "i32:0", "16\nstate:0900000005000000a55a\n"),
        ("i32:0", "i32:65536", "14\nstate:070000000b000000a55a\n"),
        ("i32:1", "i32:65536", "trap\nstate:0700000005000000a55a\n"),
    ] {
        alternatives.check(
            flags,
            "execute-memory.mjs",
            &["state:0700000005000000a55a", "--", condition, address],
            expected,
        );
    }
    let tail = ModuleFile::new(&stores_and_tail());
    tail.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "receive:i64:-1", "i32:1"],
        "return -9223372036854775808\nstate 0100000005000000a55a\n",
    );
    tail.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "receive:i64:-1", "i32:0"],
        "receive(3) 0100000002000000a55a\nreturn -1\nstate 0100000002000000a55a\n",
    );
    let continuing = ModuleFile::new(&continuation_load(false));
    continuing.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:1", "i32:65536"],
        "17\nstate:07000000a55a\n",
    );
    continuing.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:0", "i32:65536"],
        "trap\nstate:07000000a55a\n",
    );
    continuing.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:0", "i32:0"],
        "7\nstate:07000000a55a\n",
    );
    let exiting = ModuleFile::new(&conditional_result_load());
    exiting.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:0", "i32:65536"],
        "17\nstate:09000000a55a\n",
    );
    exiting.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:1", "i32:65536"],
        "trap\nstate:07000000a55a\n",
    );
    exiting.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:1", "i32:0"],
        "7\nstate:07000000a55a\n",
    );
    let captured = ModuleFile::new(&continuation_load(true));
    // Preserving this earlier read across the overlapping write forces its capture before the guard.
    captured.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:1", "i32:65536"],
        "trap\nstate:07000000a55a\n",
    );
    captured.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:1", "i32:0"],
        "17\nstate:07000000a55a\n",
    );
    captured.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:0", "i32:0"],
        "7\nstate:09000000a55a\n",
    );
    let shared = ModuleFile::new(&shared_snapshot());
    shared.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a"],
        "8\nstate:07000000a55a\n",
    );
    shared.check(
        flags,
        "execute-memory.mjs",
        &["state:05000000a55a"],
        "6\nstate:09000000a55a\n",
    );
    let sequential = ModuleFile::new(&sequential_exits());
    sequential.check(
        flags,
        "execute-memory.mjs",
        &["state:0700000005000000a55a", "--", "i32:1", "i32:65536"],
        "11\nstate:0700000005000000a55a\n",
    );
    sequential.check(
        flags,
        "execute-memory.mjs",
        &["state:0700000005000000a55a", "--", "i32:0", "i32:65536"],
        "trap\nstate:0700000005000000a55a\n",
    );
    sequential.check(
        flags,
        "execute-memory.mjs",
        &["state:0700000005000000a55a", "--", "i32:0", "i32:0"],
        "22\nstate:0700000005000000a55a\n",
    );
    sequential.check(
        flags,
        "execute-memory.mjs",
        &["state:0500000005000000a55a", "--", "i32:0", "i32:0"],
        "33\nstate:0500000009000000a55a\n",
    );
    ModuleFile::new(&constant_guard(false)).check(
        flags,
        "execute-memory.mjs",
        &["state:0700000005000000a55a"],
        "33\nstate:0100000002000000a55a\n",
    );
    ModuleFile::new(&constant_guard(true)).check(
        flags,
        "execute-memory.mjs",
        &["state:0700000005000000a55a"],
        "17\nstate:0100000005000000a55a\n",
    );
    let arm = ModuleFile::new(&branch_load_after_store());
    arm.check(
        flags,
        "execute-memory.mjs",
        &[
            "state:0700000005000000090000000b000000",
            "--",
            "i32:0",
            "i32:65536",
        ],
        "17\nstate:0100000005000000030000000b000000\n",
    );
    arm.check(
        flags,
        "execute-memory.mjs",
        &[
            "state:0700000005000000090000000b000000",
            "--",
            "i32:1",
            "i32:65536",
        ],
        "trap\nstate:0100000002000000090000000b000000\n",
    );
    arm.check(
        flags,
        "execute-memory.mjs",
        &[
            "state:0700000005000000090000000b000000",
            "--",
            "i32:1",
            "i32:12",
        ],
        "11\nstate:0100000002000000090000000b000000\n",
    );
    let fallthrough = ModuleFile::new(&fallthrough_snapshot());
    fallthrough.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:0"],
        "14\nstate:07000000a55a\n",
    );
    fallthrough.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:1"],
        "16\nstate:09000000a55a\n",
    );
    let pure = ModuleFile::new(&shared_pure_value());
    pure.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:0", "i32:41"],
        "42\nstate:07000000a55a\n",
    );
    pure.check(
        flags,
        "execute-memory.mjs",
        &["state:07000000a55a", "--", "i32:1", "i32:41"],
        "42\nstate:2a000000a55a\n",
    );
    let nested = ModuleFile::new(&nested_tail());
    nested.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "070000000500000009000000a55a",
            "receive:i64:-9223372036854775808",
            "i32:0",
            "i32:1",
        ],
        "return 17\nstate 070000000500000003000000a55a\n",
    );
    nested.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "070000000500000009000000a55a",
            "receive:i64:-9223372036854775808",
            "i32:1",
            "i32:0",
        ],
        "return 17\nstate 010000000200000003000000a55a\n",
    );
    nested.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "070000000500000009000000a55a",
            "receive:i64:-9223372036854775808",
            "i32:1",
            "i32:1",
        ],
        concat!(
            "receive(11) 010000000500000009000000a55a\n",
            "return -9223372036854775808\n",
            "state 010000000500000009000000a55a\n",
        ),
    );
    let narrow = ModuleFile::new(&narrow_condition_and_results());
    narrow.check(flags, "execute.mjs", &["run", "i32:0", "i32:255"], "0\n");
    narrow.check(flags, "execute.mjs", &["run", "i32:1", "i32:255"], "1\n");
    narrow.check(flags, "execute.mjs", &["run", "i32:1", "i32:254"], "0\n");
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn structured_branches_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn structured_branches_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
