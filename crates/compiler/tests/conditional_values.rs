#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{
    Func, FunctionImport, IntType, Mem, MemoryImport, Program, Signature, Type, I1, I32, I64, I8,
};
use wasmparser::{BlockType, Operator, Parser, Payload, ValType, Validator};

fn signature(parameters: &[Type], result: Type) -> Signature {
    Signature {
        parameters: parameters.to_vec(),
        result,
    }
}

fn memory(program: &mut Program) -> Mem {
    program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    })
}

fn callback(program: &mut Program, parameters: &[Type], result: Type) -> Func {
    program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: signature(parameters, result),
    })
}

fn compile_module(mut program: Program, run: Func) -> Vec<u8> {
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn direct_result<T: IntType>() -> Vec<u8> {
    let mut program = Program::new();
    let run = program.declare(signature(&[Type::I1, T::TYPE, T::TYPE], T::TYPE));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let yes = body.parameter::<T>(1).unwrap();
    let no = body.parameter::<T>(2).unwrap();
    let result = body
        .if_value::<T>(&condition, |arm| arm.yield_(&yes), |arm| arm.yield_(&no))
        .unwrap();
    body.return_(&result).unwrap();
    compile_module(program, run)
}

fn shared_result() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1, Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let input = body.parameter::<I32>(1).unwrap();
    let result = body
        .if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 0, 1)?;
                arm.yield_(input.add(1))
            },
            |mut arm| {
                arm.store::<I32>(state, 0, 2)?;
                arm.yield_(input.add(2))
            },
        )
        .unwrap();
    body.store(state, 4, &result).unwrap();
    body.return_(result.add(&result)).unwrap();
    compile_module(program, run)
}

fn selected_memory() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1, Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let address = body.parameter::<I32>(1).unwrap();
    body.store::<I32>(state, 0, 1).unwrap();
    let result = body
        .if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 4, 2)?;
                let value = arm.load_at::<I32>(state, &address, 0)?;
                arm.yield_(&value)
            },
            |mut arm| {
                arm.store::<I32>(state, 4, 3)?;
                arm.yield_(11)
            },
        )
        .unwrap();
    body.store::<I32>(state, 8, 4).unwrap();
    body.return_(&result).unwrap();
    compile_module(program, run)
}

fn unused_result() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = callback(&mut program, &[Type::I32], Type::I32);
    let run = program.declare(signature(&[Type::I1], Type::I32));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let _unused = body
        .if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 0, 1)?;
                let _answer = arm.call::<I32>(receive, &[9.into()])?;
                let loaded = arm.load::<I32>(state, 65536)?;
                arm.yield_(&loaded)
            },
            |mut arm| {
                arm.store::<I32>(state, 0, 2)?;
                let loaded = arm.load::<I32>(state, 65536)?;
                arm.yield_(&loaded)
            },
        )
        .unwrap();
    body.store::<I32>(state, 4, 3).unwrap();
    body.return_(17).unwrap();
    compile_module(program, run)
}

fn snapshots() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1], Type::I32));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let before = body.load::<I32>(state, 0).unwrap();
    let result = body
        .if_value::<I32>(
            &condition,
            |mut arm| {
                arm.store::<I32>(state, 0, 9)?;
                let loaded = arm.load::<I32>(state, 4)?;
                arm.yield_(loaded.add(&before))
            },
            |mut arm| {
                arm.store::<I32>(state, 4, 3)?;
                let loaded = arm.load::<I32>(state, 0)?;
                arm.yield_(loaded.add(&before))
            },
        )
        .unwrap();
    body.store::<I32>(state, 0, 12).unwrap();
    body.return_(result.add(&before)).unwrap();
    compile_module(program, run)
}

fn nested_fault() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1, Type::I1, Type::I32], Type::I64));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let fault = body.parameter::<I1>(1).unwrap();
    let address = body.parameter::<I32>(2).unwrap();
    let result = body
        .if_value::<I32>(
            &condition,
            |mut arm| {
                arm.if_(&fault, |exit| exit.return_(0x8000_0000_0000_0000u64))?;
                let loaded = arm.load_at::<I32>(state, &address, 0)?;
                arm.yield_(&loaded)
            },
            |arm| arm.yield_(11),
        )
        .unwrap();
    body.return_(result.unsigned().extend::<I64>()).unwrap();
    compile_module(program, run)
}

fn narrow_result() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = callback(&mut program, &[Type::I8, Type::I8], Type::I64);
    let run = program.declare(signature(&[Type::I1, Type::I8], Type::I64));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let input = body.parameter::<I8>(1).unwrap();
    let result = body
        .if_value::<I8>(
            &condition,
            |arm| arm.yield_(input.add(1)),
            |arm| arm.yield_(7),
        )
        .unwrap();
    body.store(state, 0, &result).unwrap();
    body.tail_call(receive, &[result.argument(), result.argument()])
        .unwrap();
    compile_module(program, run)
}

fn predicate_result() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1, Type::I1], Type::I1));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let input = body.parameter::<I1>(1).unwrap();
    let result = body
        .if_value::<I1>(
            &condition,
            |arm| arm.yield_(input.add(1)),
            |arm| arm.yield_(false),
        )
        .unwrap();
    body.if_(&result, |mut arm| arm.store::<I32>(state, 0, 9))
        .unwrap();
    body.return_(&result).unwrap();
    compile_module(program, run)
}

fn nested_result() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1, Type::I1, Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let outer = body.parameter::<I1>(0).unwrap();
    let inner = body.parameter::<I1>(1).unwrap();
    let shared = body.parameter::<I32>(2).unwrap().add(1);
    let result = body
        .if_value::<I32>(
            &outer,
            |mut arm| {
                let result = arm.if_value::<I32>(
                    &inner,
                    |child| child.yield_(shared.add(1)),
                    |child| child.yield_(shared.add(2)),
                )?;
                arm.yield_(&result)
            },
            |arm| arm.yield_(shared.add(3)),
        )
        .unwrap();
    body.store(state, 0, &shared).unwrap();
    body.return_(result.add(&shared)).unwrap();
    compile_module(program, run)
}

fn returning_arm() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1], Type::I64));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let result = body
        .if_value::<I32>(
            &condition,
            |arm| arm.return_(0x8000_0000_0000_0000u64),
            |arm| arm.yield_(7),
        )
        .unwrap();
    body.store::<I32>(state, 0, 9).unwrap();
    body.return_(result.unsigned().extend::<I64>()).unwrap();
    compile_module(program, run)
}

fn tailing_arm() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = callback(&mut program, &[Type::I32], Type::I64);
    let run = program.declare(signature(&[Type::I1], Type::I64));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let result = body
        .if_value::<I32>(
            &condition,
            |arm| arm.yield_(11),
            |mut arm| {
                arm.store::<I32>(state, 0, 1)?;
                arm.tail_call(receive, &[9.into()])
            },
        )
        .unwrap();
    body.store::<I32>(state, 4, 2).unwrap();
    body.return_(result.unsigned().extend::<I64>()).unwrap();
    compile_module(program, run)
}

#[derive(Debug, PartialEq)]
enum Event {
    If(BlockType),
    Else,
    End,
    Load(u64),
    Store(u64),
    Mask,
    Call,
    Tail,
    Return,
}

#[derive(Default, Debug)]
struct Code {
    events: Vec<Event>,
    locals: u32,
    writes: usize,
    adds: usize,
    drops: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        if let Payload::CodeSectionEntry(body) = payload.unwrap() {
            bodies += 1;
            for local in body.get_locals_reader().unwrap() {
                code.locals += local.unwrap().0;
            }
            let mut reader = body.get_operators_reader().unwrap();
            while !reader.eof() {
                let event = match reader.read().unwrap() {
                    Operator::If { blockty } => Some(Event::If(blockty)),
                    Operator::Else => Some(Event::Else),
                    Operator::End if !reader.eof() => Some(Event::End),
                    Operator::I32Load { memarg } => Some(Event::Load(memarg.offset)),
                    Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                        Some(Event::Store(memarg.offset))
                    }
                    Operator::I32And => Some(Event::Mask),
                    Operator::Call { .. } => Some(Event::Call),
                    Operator::ReturnCall { .. } => Some(Event::Tail),
                    Operator::Return => Some(Event::Return),
                    Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                        code.writes += 1;
                        None
                    }
                    Operator::I32Add | Operator::I64Add => {
                        code.adds += 1;
                        None
                    }
                    Operator::Drop => {
                        code.drops += 1;
                        None
                    }
                    _ => None,
                };
                code.events.extend(event);
            }
        }
    }
    assert_eq!(bodies, 1);
    code
}

#[test]
fn value_arms_use_the_result_stack_before_the_join_is_saved() {
    for (bytes, carrier) in [
        (direct_result::<I32>(), ValType::I32),
        (direct_result::<I64>(), ValType::I64),
    ] {
        let code = inspect(&bytes);
        assert_eq!(code.locals, 1);
        assert_eq!(code.writes, 1);
        assert_eq!(
            code.events,
            [
                Event::If(BlockType::Type(carrier)),
                Event::Else,
                Event::End,
                Event::Return
            ]
        );
    }
}

#[test]
fn shared_join_outputs_are_saved_after_one_selected_arm() {
    let code = inspect(&shared_result());
    assert_eq!(code.locals, 1);
    assert_eq!(code.writes, 1);
    assert_eq!(code.adds, 3);
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Store(0),
            Event::Else,
            Event::Store(0),
            Event::End,
            Event::Store(4),
            Event::Return,
        ]
    );
}

#[test]
fn selected_loads_follow_arm_stores_and_precede_continuation_stores() {
    assert_eq!(
        inspect(&selected_memory()).events,
        [
            Event::Store(0),
            Event::If(BlockType::Type(ValType::I32)),
            Event::Store(4),
            Event::Load(0),
            Event::Else,
            Event::Store(4),
            Event::End,
            Event::Store(8),
            Event::Return,
        ]
    );
}

#[test]
fn unused_join_values_drop_reads_but_preserve_branch_effects() {
    let code = inspect(&unused_result());
    assert_eq!(code.drops, 1);
    assert_eq!(code.locals, 0);
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Empty),
            Event::Store(0),
            Event::Call,
            Event::Else,
            Event::Store(0),
            Event::End,
            Event::Store(4),
            Event::Return,
        ]
    );
}

#[test]
fn prior_snapshots_survive_writes_in_either_arm_and_the_continuation() {
    assert_eq!(
        inspect(&snapshots()).events,
        [
            Event::Load(0),
            Event::If(BlockType::Type(ValType::I32)),
            Event::Store(0),
            Event::Load(4),
            Event::Else,
            Event::Store(4),
            Event::Load(0),
            Event::End,
            Event::Store(0),
            Event::Return,
        ]
    );
}

#[test]
fn narrow_joins_normalize_at_observers_instead_of_arm_exits() {
    let code = inspect(&narrow_result());
    assert_eq!(code.adds, 1);
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Else,
            Event::End,
            Event::Store(0),
            Event::Mask,
            Event::Tail,
        ]
    );
    let code = inspect(&predicate_result());
    assert_eq!(
        code.events
            .iter()
            .filter(|event| **event == Event::Mask)
            .count(),
        1
    );
}

#[test]
fn nested_joins_share_parent_values_without_repeating_arithmetic() {
    let code = inspect(&nested_result());
    assert_eq!(code.locals, 2);
    assert_eq!(code.writes, 2);
    assert_eq!(code.adds, 5);
    assert_eq!(
        code.events
            .iter()
            .filter(|event| matches!(event, Event::If(_)))
            .count(),
        2
    );
}

#[test]
fn function_exits_inside_value_arms_keep_the_function_result_type() {
    let code = inspect(&nested_fault());
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::If(BlockType::Empty),
            Event::Return,
            Event::End,
            Event::Load(0),
            Event::Else,
            Event::End,
            Event::Return,
        ]
    );
    let code = inspect(&returning_arm());
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Return,
            Event::Else,
            Event::End,
            Event::Store(0),
            Event::Return,
        ]
    );
    let code = inspect(&tailing_arm());
    assert_eq!(
        code.events,
        [
            Event::If(BlockType::Type(ValType::I32)),
            Event::Else,
            Event::Store(0),
            Event::Tail,
            Event::End,
            Event::Store(4),
            Event::Return,
        ]
    );
}

fn check_execution(flags: &[&str]) {
    let direct = ModuleFile::new(&direct_result::<I32>());
    direct.check(
        flags,
        "execute-tail.mjs",
        &["run", "-", "", "i32:1", "i32:-2147483648", "i32:17"],
        "return -2147483648\n",
    );
    direct.check(
        flags,
        "execute-tail.mjs",
        &["run", "-", "", "i32:0", "i32:-2147483648", "i32:17"],
        "return 17\n",
    );
    let wide = ModuleFile::new(&direct_result::<I64>());
    wide.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "-",
            "",
            "i32:1",
            "i64:-9223372036854775808",
            "i64:9223372036854775807",
        ],
        "return -9223372036854775808\n",
    );
    wide.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "-",
            "",
            "i32:0",
            "i64:-9223372036854775808",
            "i64:9223372036854775807",
        ],
        "return 9223372036854775807\n",
    );
    let shared = ModuleFile::new(&shared_result());
    shared.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "", "i32:1", "i32:2147483647"],
        "return 0\nstate 0100000000000080a55a\n",
    );
    shared.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "", "i32:0", "i32:4"],
        "return 12\nstate 0200000006000000a55a\n",
    );
    let selected = ModuleFile::new(&selected_memory());
    selected.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000050000000b000000a55a", "", "i32:1", "i32:8"],
        "return 11\nstate 010000000200000004000000a55a\n",
    );
    selected.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "07000000050000000b000000a55a",
            "",
            "i32:1",
            "i32:65536",
        ],
        "return trap\nstate 01000000020000000b000000a55a\n",
    );
    selected.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "07000000050000000b000000a55a",
            "",
            "i32:0",
            "i32:65536",
        ],
        "return 11\nstate 010000000300000004000000a55a\n",
    );
    let unused = ModuleFile::new(&unused_result());
    unused.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "receive:i32:23", "i32:1"],
        "receive(9) 0100000005000000a55a\nreturn 17\nstate 0100000003000000a55a\n",
    );
    unused.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "receive:i32:23", "i32:0"],
        "return 17\nstate 0200000003000000a55a\n",
    );
    let snapshots = ModuleFile::new(&snapshots());
    snapshots.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "", "i32:1"],
        "return 19\nstate 0c00000005000000a55a\n",
    );
    snapshots.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000a55a", "", "i32:0"],
        "return 21\nstate 0c00000003000000a55a\n",
    );
    let fault = ModuleFile::new(&nested_fault());
    fault.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1", "i32:1", "i32:65536"],
        "return -9223372036854775808\nstate 07000000a55a\n",
    );
    fault.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1", "i32:0", "i32:0"],
        "return 7\nstate 07000000a55a\n",
    );
    fault.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:0", "i32:1", "i32:65536"],
        "return 11\nstate 07000000a55a\n",
    );
    fault.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1", "i32:0", "i32:65536"],
        "return trap\nstate 07000000a55a\n",
    );
    let narrow = ModuleFile::new(&narrow_result());
    narrow.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "a55a",
            "receive:i64:-9223372036854775808",
            "i32:1",
            "i32:255",
        ],
        "receive(0,0) 005a\nreturn -9223372036854775808\nstate 005a\n",
    );
    narrow.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "a55a",
            "receive:i64:-9223372036854775808",
            "i32:0",
            "i32:255",
        ],
        "receive(7,7) 075a\nreturn -9223372036854775808\nstate 075a\n",
    );
    let predicate = ModuleFile::new(&predicate_result());
    predicate.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1", "i32:1"],
        "return 0\nstate 07000000a55a\n",
    );
    predicate.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1", "i32:0"],
        "return 1\nstate 09000000a55a\n",
    );
    predicate.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:0", "i32:0"],
        "return 0\nstate 07000000a55a\n",
    );
    let nested = ModuleFile::new(&nested_result());
    nested.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1", "i32:1", "i32:4"],
        "return 11\nstate 05000000a55a\n",
    );
    nested.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1", "i32:0", "i32:4"],
        "return 12\nstate 05000000a55a\n",
    );
    nested.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:0", "i32:1", "i32:4"],
        "return 13\nstate 05000000a55a\n",
    );
    let returning = ModuleFile::new(&returning_arm());
    returning.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:1"],
        "return -9223372036854775808\nstate 07000000a55a\n",
    );
    returning.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000a55a", "", "i32:0"],
        "return 7\nstate 09000000a55a\n",
    );
    let tailing = ModuleFile::new(&tailing_arm());
    tailing.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "0700000005000000a55a",
            "receive:i64:-9223372036854775808",
            "i32:0",
        ],
        concat!(
            "receive(9) 0100000005000000a55a\n",
            "return -9223372036854775808\n",
            "state 0100000005000000a55a\n"
        ),
    );
    tailing.check(
        flags,
        "execute-tail.mjs",
        &[
            "run",
            "0700000005000000a55a",
            "receive:i64:-9223372036854775808",
            "i32:1",
        ],
        "return 11\nstate 0700000002000000a55a\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn conditional_values_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn conditional_values_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
