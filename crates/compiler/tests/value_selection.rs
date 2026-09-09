#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{
    FunctionBuilder, IntType, Mem, MemoryImport, Program, Signature, Type, Val, I1, I32, I64, I8,
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
    let run = program.declare(Signature {
        parameters: parameters.to_vec(),
        result: Some(T::TYPE),
    });
    let mut body = program.define(run).unwrap();
    let value = build(&mut body, state);
    body.return_(value).unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn selected_loads(body: &mut FunctionBuilder<'_>, state: Mem) -> Val<I32> {
    let condition = body.parameter::<I1>(0).unwrap();
    let left = body.parameter::<I32>(1).unwrap();
    let right = body.parameter::<I32>(2).unwrap();
    let left = body.load_at::<I32>(state, left, 0).unwrap();
    let right = body.load_at::<I32>(state, right, 0).unwrap();
    condition.select(left, right)
}

fn eager_loads() -> Vec<u8> {
    module(&[Type::I1, Type::I32, Type::I32], selected_loads)
}

fn lazy_loads() -> Vec<u8> {
    module(&[Type::I1, Type::I32, Type::I32], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let left = body.parameter::<I32>(1).unwrap();
        let right = body.parameter::<I32>(2).unwrap();
        body.if_value::<I32>(
            condition,
            |mut arm| {
                let value = arm.load_at::<I32>(state, left, 0)?;
                arm.yield_(value)
            },
            |mut arm| {
                let value = arm.load_at::<I32>(state, right, 0)?;
                arm.yield_(value)
            },
        )
        .unwrap()
    })
}

fn unused_selection() -> Vec<u8> {
    module(&[Type::I1, Type::I32, Type::I32], |body, state| {
        let _unused = selected_loads(body, state);
        body.value::<I32>(17).unwrap()
    })
}

fn exit_selection() -> Vec<u8> {
    module(&[Type::I1, Type::I1, Type::I32], |body, _| {
        let condition = body.parameter::<I1>(0).unwrap();
        let exit = body.parameter::<I1>(1).unwrap();
        let value = body.parameter::<I32>(2).unwrap();
        let selected = condition.select(value.add(1), value.add(2));
        body.if_(exit, |arm| arm.return_(selected)).unwrap();
        body.value::<I32>(17).unwrap()
    })
}

fn store_snapshot() -> Vec<u8> {
    module(&[Type::I1], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let previous = body.load::<I32>(state, 0).unwrap();
        let selected = condition.select(previous, 5);
        body.store::<I32>(state, 0, 9).unwrap();
        selected
    })
}

fn call_snapshot() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let mutate = program.declare(Signature {
        parameters: vec![],
        result: Some(Type::I32),
    });
    let mut body = program.define(mutate).unwrap();
    body.store::<I32>(state, 0, 9).unwrap();
    body.return_(11).unwrap();
    let run = program.declare(Signature {
        parameters: vec![Type::I1],
        result: Some(Type::I32),
    });
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let previous = body.load::<I32>(state, 0).unwrap();
    let selected = condition.select(previous, 5);
    let _unused = body.call::<I32>(mutate, &[]).unwrap();
    body.return_(selected).unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn narrow_selection() -> Vec<u8> {
    module(&[Type::I1, Type::I8], |body, state| {
        let condition = body.parameter::<I1>(0).unwrap();
        let value = body.parameter::<I8>(1).unwrap();
        let selected = condition.select(value.add(1), value.add(2));
        body.store(state, 0, &selected).unwrap();
        selected
    })
}

fn wide_selection() -> Vec<u8> {
    module(&[Type::I1, Type::I64, Type::I64], |body, _| {
        body.parameter::<I1>(0).unwrap().select(
            body.parameter::<I64>(1).unwrap(),
            body.parameter::<I64>(2).unwrap(),
        )
    })
}

#[derive(Debug, PartialEq, Eq)]
enum Event {
    Load,
    Store,
    Call,
    Select,
    If,
    Else,
    End,
    Add,
    And,
    Return,
}

fn inspect(bytes: &[u8]) -> Vec<Vec<Event>> {
    Validator::new().validate_all(bytes).unwrap();
    let mut functions = Vec::new();
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            let mut events = Vec::new();
            let mut depth = 0;
            for operator in body.get_operators_reader().unwrap() {
                let event = match operator.unwrap() {
                    Operator::I32Load { .. } => Event::Load,
                    Operator::I32Store { .. } | Operator::I32Store8 { .. } => Event::Store,
                    Operator::Call { .. } => Event::Call,
                    Operator::Select => Event::Select,
                    Operator::If { .. } => {
                        depth += 1;
                        Event::If
                    }
                    Operator::Else => Event::Else,
                    Operator::End if depth > 0 => {
                        depth -= 1;
                        Event::End
                    }
                    Operator::I32Add => Event::Add,
                    Operator::I32And => Event::And,
                    Operator::Return => Event::Return,
                    _ => continue,
                };
                events.push(event);
            }
            functions.push(events);
        }
    }
    functions
}

#[test]
fn select_evaluates_both_loads_while_conditional_arms_are_lazy() {
    use Event::*;
    assert_eq!(inspect(&eager_loads()), [vec![Load, Load, Select, Return]]);
    assert_eq!(
        inspect(&lazy_loads()),
        [vec![If, Load, Else, Load, End, Return]]
    );
}

#[test]
fn unused_selection_omits_its_loads() {
    assert_eq!(inspect(&unused_selection()), [vec![Event::Return]]);
}

#[test]
fn selection_used_only_by_an_exit_is_evaluated_inside_it() {
    use Event::*;
    assert_eq!(
        inspect(&exit_selection()),
        [vec![If, Add, Add, Select, Return, End, Return]]
    );
}

#[test]
fn selected_loads_keep_their_snapshot_across_stores_and_calls() {
    use Event::*;
    assert_eq!(
        inspect(&store_snapshot()),
        [vec![Load, Store, Select, Return]]
    );
    assert_eq!(
        inspect(&call_snapshot()),
        [vec![Store, Return], vec![Load, Call, Select, Return]]
    );
}

#[test]
fn shared_narrow_selection_is_normalized_at_return_after_the_raw_store() {
    use Event::*;
    assert_eq!(
        inspect(&narrow_selection()),
        [vec![Add, Add, Select, Store, And, Return]]
    );
}

fn check_execution(flags: &[&str]) {
    let eager = ModuleFile::new(&eager_loads());
    let lazy = ModuleFile::new(&lazy_loads());
    for arguments in [
        [
            "state:0700000009000000",
            "--",
            "i32:1",
            "i32:0",
            "i32:65536",
        ],
        [
            "state:0700000009000000",
            "--",
            "i32:0",
            "i32:65536",
            "i32:0",
        ],
    ] {
        eager.check(
            flags,
            "execute-memory.mjs",
            &arguments,
            "trap\nstate:0700000009000000\n",
        );
        lazy.check(
            flags,
            "execute-memory.mjs",
            &arguments,
            "7\nstate:0700000009000000\n",
        );
    }
    eager.check(
        flags,
        "execute-memory.mjs",
        &["state:0700000009000000", "--", "i32:0", "i32:0", "i32:4"],
        "9\nstate:0700000009000000\n",
    );
    ModuleFile::new(&unused_selection()).check(
        flags,
        "execute-memory.mjs",
        &["state:07000000", "--", "i32:1", "i32:65536", "i32:65536"],
        "17\nstate:07000000\n",
    );
    let exit = ModuleFile::new(&exit_selection());
    for (args, expected) in [
        (["run", "i32:1", "i32:1", "i32:41"], "42\n"),
        (["run", "i32:0", "i32:1", "i32:41"], "43\n"),
        (["run", "i32:1", "i32:0", "i32:41"], "17\n"),
    ] {
        exit.check(flags, "execute.mjs", &args, expected);
    }
    for bytes in [store_snapshot(), call_snapshot()] {
        let module = ModuleFile::new(&bytes);
        module.check(
            flags,
            "execute-memory.mjs",
            &["state:07000000a55a", "--", "i32:1"],
            "7\nstate:09000000a55a\n",
        );
        module.check(
            flags,
            "execute-memory.mjs",
            &["state:07000000a55a", "--", "i32:0"],
            "5\nstate:09000000a55a\n",
        );
    }
    let narrow = ModuleFile::new(&narrow_selection());
    narrow.check(
        flags,
        "execute-memory.mjs",
        &["state:075a", "--", "i32:1", "i32:255"],
        "0\nstate:005a\n",
    );
    narrow.check(
        flags,
        "execute-memory.mjs",
        &["state:075a", "--", "i32:0", "i32:255"],
        "1\nstate:015a\n",
    );
    let wide = ModuleFile::new(&wide_selection());
    wide.check(
        flags,
        "execute.mjs",
        &[
            "run",
            "i32:1",
            "i64:-9223372036854775808",
            "i64:9223372036854775807",
        ],
        "-9223372036854775808\n",
    );
    wide.check(
        flags,
        "execute.mjs",
        &[
            "run",
            "i32:0",
            "i64:-9223372036854775808",
            "i64:9223372036854775807",
        ],
        "9223372036854775807\n",
    );
}

#[test]
#[ignore = "requires Node.js with Wasm support"]
fn value_selection_executes_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js with Wasm support"]
fn value_selection_executes_in_optimizing_v8() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
