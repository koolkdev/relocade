#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

use wasm86_compiler::{
    BuildError, Func, FunctionBuilder, FunctionImport, IntType, Mem, MemoryImport, Program,
    Signature, Type, Val, I1, I32, I64, I8,
};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

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

fn callback(program: &mut Program) -> Func {
    program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: signature(&[Type::I32], Type::I32),
    })
}

fn compile_module(mut program: Program, run: Func) -> Vec<u8> {
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

fn module<T: IntType>(
    parameters: &[Type],
    build: impl FnOnce(&mut FunctionBuilder<'_>, Mem) -> Val<T>,
) -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(parameters, T::TYPE));
    let mut body = program.define(run).unwrap();
    let result = build(&mut body, state);
    body.return_(result).unwrap();
    compile_module(program, run)
}

fn dense_values() -> Vec<u8> {
    module(&[Type::I32], |body, _| {
        let selector = body.parameter::<I32>(0).unwrap();
        body.switch_value::<I32, _>(&selector, &[10, 11, 12, 13], |arm, key| {
            arm.yield_(match key {
                Some(10) => 41,
                Some(11) => 43,
                Some(12) => 47,
                Some(13) => 53,
                _ => 97,
            })
        })
        .unwrap()
    })
}

fn sparse_endpoints() -> Vec<u8> {
    module(&[Type::I32], |body, _| {
        let selector = body.parameter::<I32>(0).unwrap();
        body.switch_value::<I32, _>(&selector, &[0xffff_ffff, 0, 0x8000_0000], |arm, key| {
            arm.yield_(match key {
                Some(0) => 11,
                Some(0x8000_0000) => 13,
                Some(0xffff_ffff) => 17,
                _ => 19,
            })
        })
        .unwrap()
    })
}

fn narrow_selector() -> Vec<u8> {
    module(&[Type::I8], |body, _| {
        let selector = body.parameter::<I8>(0).unwrap().add(1);
        body.switch_value::<I32, _>(&selector, &[0, 1, 255], |arm, key| {
            arm.yield_(match key {
                Some(0) => 42,
                Some(1) => 43,
                Some(255) => 47,
                _ => 53,
            })
        })
        .unwrap()
    })
}

fn narrow_result() -> Vec<u8> {
    module(&[Type::I32, Type::I8], |body, state| {
        let selector = body.parameter::<I32>(0).unwrap();
        let input = body.parameter::<I8>(1).unwrap();
        let result = body
            .switch_value::<I8, _>(selector, &[0, 1], |arm, key| {
                arm.yield_(input.add(if key == Some(0) { 1 } else { 2 }))
            })
            .unwrap();
        body.store(state, 0, &result).unwrap();
        result
    })
}

fn nested_value_and_exit() -> Vec<u8> {
    module(&[Type::I32, Type::I1, Type::I64], |body, _| {
        let selector = body.parameter::<I32>(0).unwrap();
        let condition = body.parameter::<I1>(1).unwrap();
        let input = body.parameter::<I64>(2).unwrap();
        body.switch_value::<I64, _>(selector, &[0, 1], |mut arm, key| {
            if key == Some(0) {
                let nested = arm.if_value::<I64>(
                    &condition,
                    |branch| branch.yield_(&input),
                    |branch| branch.yield_(0x8000_0000_0000_0000u64),
                )?;
                arm.yield_(nested)
            } else {
                arm.return_(0xffff_ffff_ffff_ffffu64)
            }
        })
        .unwrap()
    })
}

fn selected_effects_and_snapshot() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = callback(&mut program);
    let mutate = program.declare(signature(&[], Type::I32));
    let mut mutation = program.define(mutate).unwrap();
    mutation.store::<I32>(state, 0, 13).unwrap();
    mutation.return_(99).unwrap();
    let run = program.declare(signature(&[Type::I32, Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let address = body.parameter::<I32>(1).unwrap();
    let before = body.load::<I32>(state, 0).unwrap();
    body.switch(&selector, &[0, 1, 2], |mut arm, key| match key {
        Some(0) => arm.store::<I32>(state, 0, 9),
        Some(1) => {
            let value = arm.load_at::<I32>(state, &address, 0)?;
            arm.store(state, 4, value)
        }
        Some(2) => {
            let _unused = arm.call::<I32>(mutate, &[])?;
            Ok(())
        }
        _ => arm.tail_call(receive, &[before.argument()]),
    })
    .unwrap();
    let after = body.load::<I32>(state, 0).unwrap();
    body.store::<I32>(state, 8, 11).unwrap();
    body.return_(before.add(after)).unwrap();
    compile_module(program, run)
}

fn shared_call_result() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = callback(&mut program);
    let run = program.declare(signature(&[Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let result = body
        .switch_value::<I32, _>(selector, &[0, 1], |mut arm, key| {
            arm.store::<I32>(state, 0, key.map_or(3, |key| key + 1))?;
            let answer = arm.call::<I32>(receive, &[9.into()])?;
            arm.yield_(answer.add(1))
        })
        .unwrap();
    body.store(state, 4, &result).unwrap();
    body.return_(result.add(&result)).unwrap();
    compile_module(program, run)
}

fn unused_result_keeps_effects() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = callback(&mut program);
    let run = program.declare(signature(&[Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let _unused = body
        .switch_value::<I32, _>(selector, &[1, 2], |mut arm, key| {
            arm.store::<I32>(state, 0, key.unwrap_or(3))?;
            if key == Some(1) {
                let _answer = arm.call::<I32>(receive, &[9.into()])?;
            }
            let unused_load = arm.load::<I32>(state, 65536)?;
            arm.yield_(unused_load)
        })
        .unwrap();
    body.return_(17).unwrap();
    compile_module(program, run)
}

fn empty_default() -> Vec<u8> {
    module(&[Type::I32], |body, _| {
        let selector = body.parameter::<I32>(0).unwrap();
        let mut visits = 0;
        let value = body
            .switch_value::<I32, _>(selector, &[], |arm, key| {
                assert_eq!(key, None);
                visits += 1;
                arm.yield_(23)
            })
            .unwrap();
        assert_eq!(visits, 1);
        value
    })
}

fn failed_switch_discards_effects() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let mut visited_default = false;
    let failed = body.switch(selector, &[0, 1], |mut arm, key| match key {
        Some(0) => {
            let receive = callback(arm.program());
            let _answer = arm.call::<I32>(receive, &[9.into()])?;
            arm.store::<I32>(state, 0, 9)
        }
        Some(1) => arm.return_(9u64),
        _ => {
            visited_default = true;
            Ok(())
        }
    });
    assert_eq!(
        failed.err(),
        Some(BuildError::TypeMismatch {
            expected: Type::I32,
            actual: Type::I64,
        })
    );
    assert!(!visited_default);
    body.store::<I32>(state, 4, 17).unwrap();
    body.return_(31).unwrap();
    compile_module(program, run)
}

#[derive(Debug, PartialEq)]
enum Event {
    Load(u64),
    Store(u64),
    Table(usize),
    Call,
    Tail,
}

fn inspect_run(bytes: &[u8]) -> Vec<Event> {
    Validator::new().validate_all(bytes).unwrap();
    let mut imports = 0;
    let mut run = None;
    let mut functions = Vec::new();
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
                        run = Some(export.index as usize);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let mut events = Vec::new();
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::I32Load { memarg } => events.push(Event::Load(memarg.offset)),
                        Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                            events.push(Event::Store(memarg.offset))
                        }
                        Operator::BrTable { targets } => {
                            events.push(Event::Table(targets.len() as usize))
                        }
                        Operator::Call { .. } => events.push(Event::Call),
                        Operator::ReturnCall { .. } => events.push(Event::Tail),
                        _ => {}
                    }
                }
                functions.push(events);
            }
            _ => {}
        }
    }
    functions.remove(run.unwrap() - imports)
}

#[test]
fn dense_dispatch_uses_a_table_and_sparse_endpoints_remain_bounded() {
    assert!(inspect_run(&dense_values()).contains(&Event::Table(4)));
    let sparse = sparse_endpoints();
    inspect_run(&sparse);
    assert!(
        sparse.len() < 2048,
        "sparse keys must not allocate their numeric span"
    );
}

#[test]
fn switches_capture_prior_reads_across_selected_stores_and_calls() {
    let events = inspect_run(&selected_effects_and_snapshot());
    let dispatch = events
        .iter()
        .position(|event| matches!(event, Event::Table(_)))
        .unwrap();
    assert_eq!(events[0], Event::Load(0));
    assert!(events[dispatch + 1..].contains(&Event::Store(0)));
    assert!(events[dispatch + 1..].contains(&Event::Call));
    assert!(events[dispatch + 1..].contains(&Event::Tail));
}

#[test]
fn unused_switch_results_drop_loads_but_retain_selected_effects() {
    let events = inspect_run(&unused_result_keeps_effects());
    assert!(!events.iter().any(|event| matches!(event, Event::Load(_))));
    assert!(events.contains(&Event::Call));
    assert!(events.contains(&Event::Store(0)));
}

#[test]
fn typed_and_nested_switch_results_form_valid_modules() {
    for bytes in [
        narrow_selector(),
        narrow_result(),
        nested_value_and_exit(),
        shared_call_result(),
    ] {
        inspect_run(&bytes);
    }
}

#[test]
fn values_from_completed_switch_arms_cannot_escape_their_scope() {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let mut child = None;
    body.switch(selector, &[0], |mut arm, key| {
        if key == Some(0) {
            child = Some(arm.load::<I32>(state, 0)?);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(
        body.value::<I32>(child.unwrap()).err(),
        Some(BuildError::OutOfScope)
    );
    body.return_(17).unwrap();
    inspect_run(&compile_module(program, run));
}

#[test]
fn empty_cases_build_only_the_default_and_need_no_table() {
    let events = inspect_run(&empty_default());
    assert!(!events.iter().any(|event| matches!(event, Event::Table(_))));
}

#[test]
fn selectors_and_keys_are_validated_before_arm_construction() {
    let mut program = Program::new();
    let run = program.declare(signature(&[Type::I8], Type::I32));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I8>(0).unwrap();
    let mut calls = 0;
    assert_eq!(
        body.switch(&selector, &[1, 1], |_, _| {
            calls += 1;
            Ok(())
        })
        .err(),
        Some(BuildError::DuplicateSwitchCase { key: 1 })
    );
    assert_eq!(
        body.switch(&selector, &[256], |_, _| {
            calls += 1;
            Ok(())
        })
        .err(),
        Some(BuildError::SwitchCaseOutOfRange {
            key: 256,
            selector: Type::I8
        })
    );
    let mut foreign_program = Program::new();
    let foreign_run = foreign_program.declare(signature(&[Type::I32], Type::I32));
    let foreign_body = foreign_program.define(foreign_run).unwrap();
    let foreign = foreign_body.parameter::<I32>(0).unwrap();
    assert_eq!(
        body.switch(foreign, &[1, 1], |_, _| {
            calls += 1;
            Ok(())
        })
        .err(),
        Some(BuildError::ForeignBody)
    );
    assert_eq!(calls, 0);
    foreign_body.return_(0).unwrap();
    body.return_(17).unwrap();
    inspect_run(&compile_module(program, run));
}

#[test]
fn value_switches_require_a_yield_and_completed_arms() {
    let mut program = Program::new();
    let run = program.declare(signature(&[Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let selector = body.parameter::<I32>(0).unwrap();
    let incomplete = body.switch_value::<I32, _>(&selector, &[0], |arm, key| {
        if key == Some(0) {
            arm.yield_(7)
        } else {
            Ok(())
        }
    });
    assert_eq!(incomplete.err(), Some(BuildError::IncompleteBranch));
    let no_value = body.switch_value::<I8, _>(&selector, &[0], |arm, _| arm.return_(11));
    assert_eq!(no_value.err(), Some(BuildError::MissingBranchValue));
    assert_eq!(
        body.switch(selector, &[0], |arm, _| arm.yield_(7)).err(),
        Some(BuildError::InvalidYield)
    );
    body.return_(17).unwrap();
    inspect_run(&compile_module(program, run));
}

#[test]
fn a_later_arm_failure_removes_earlier_effects_and_unused_imports() {
    let bytes = failed_switch_discards_effects();
    assert_eq!(inspect_run(&bytes), [Event::Store(4)]);
    for payload in Parser::new(0).parse_all(&bytes) {
        if let Payload::ImportSection(section) = payload.unwrap() {
            for import in section {
                assert!(!matches!(import.unwrap().ty, TypeRef::Func(_)));
            }
        }
    }
}

fn check_execution(flags: &[&str]) {
    let empty = ModuleFile::new(&empty_default());
    empty.check(
        flags,
        "execute-tail.mjs",
        &["run", "-", "", "i32:-1"],
        "return 23\n",
    );
    let failed = ModuleFile::new(&failed_switch_discards_effects());
    failed.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000", "", "i32:0"],
        "return 31\nstate 0700000011000000\n",
    );
    let dense = ModuleFile::new(&dense_values());
    for (input, result) in [
        ("i32:10", 41),
        ("i32:11", 43),
        ("i32:12", 47),
        ("i32:13", 53),
        ("i32:9", 97),
        ("i32:14", 97),
        ("i32:-1", 97),
    ] {
        dense.check(
            flags,
            "execute-tail.mjs",
            &["run", "-", "", input],
            &format!("return {result}\n"),
        );
    }
    let sparse = ModuleFile::new(&sparse_endpoints());
    for (input, result) in [
        ("i32:0", 11),
        ("i32:-2147483648", 13),
        ("i32:-1", 17),
        ("i32:2147483647", 19),
        ("i32:1", 19),
    ] {
        sparse.check(
            flags,
            "execute-tail.mjs",
            &["run", "-", "", input],
            &format!("return {result}\n"),
        );
    }
    let selector = ModuleFile::new(&narrow_selector());
    for (input, result) in [
        ("i32:255", 42),
        ("i32:254", 47),
        ("i32:0", 43),
        ("i32:1", 53),
    ] {
        selector.check(
            flags,
            "execute-tail.mjs",
            &["run", "-", "", input],
            &format!("return {result}\n"),
        );
    }
    let narrow = ModuleFile::new(&narrow_result());
    narrow.check(
        flags,
        "execute-tail.mjs",
        &["run", "a5a5a5a5", "", "i32:0", "i32:255"],
        "return 0\nstate 00a5a5a5\n",
    );
    narrow.check(
        flags,
        "execute-tail.mjs",
        &["run", "a5a5a5a5", "", "i32:2", "i32:255"],
        "return 1\nstate 01a5a5a5\n",
    );
    let nested = ModuleFile::new(&nested_value_and_exit());
    for (selector, condition, result) in [
        ("i32:0", "i32:1", "9223372036854775807"),
        ("i32:0", "i32:0", "-9223372036854775808"),
        ("i32:1", "i32:0", "-1"),
        ("i32:2", "i32:1", "-1"),
    ] {
        nested.check(
            flags,
            "execute-tail.mjs",
            &[
                "run",
                "-",
                "",
                selector,
                condition,
                "i64:9223372036854775807",
            ],
            &format!("return {result}\n"),
        );
    }
    let effects = ModuleFile::new(&selected_effects_and_snapshot());
    for (selector, address, expected) in [
        (
            "i32:0",
            "i32:65536",
            "return 16\nstate 09000000050000000b000000\n",
        ),
        (
            "i32:1",
            "i32:0",
            "return 14\nstate 07000000070000000b000000\n",
        ),
        (
            "i32:1",
            "i32:65536",
            "return trap\nstate 070000000500000006000000\n",
        ),
        (
            "i32:2",
            "i32:65536",
            "return 20\nstate 0d000000050000000b000000\n",
        ),
        (
            "i32:3",
            "i32:65536",
            "receive(7) 070000000500000006000000\nreturn 41\nstate 070000000500000006000000\n",
        ),
    ] {
        effects.check(
            flags,
            "execute-tail.mjs",
            &[
                "run",
                "070000000500000006000000",
                "receive:i32:41",
                selector,
                address,
            ],
            expected,
        );
    }
    let shared = ModuleFile::new(&shared_call_result());
    shared.check(
        flags,
        "execute-tail.mjs",
        &["run", "0700000005000000", "receive:i32:23", "i32:1"],
        concat!(
            "receive(9) 0200000005000000\n",
            "return 48\nstate 0200000018000000\n"
        ),
    );
    let unused = ModuleFile::new(&unused_result_keeps_effects());
    unused.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000", "receive:i32:23", "i32:1"],
        "receive(9) 01000000\nreturn 17\nstate 01000000\n",
    );
    unused.check(
        flags,
        "execute-tail.mjs",
        &["run", "07000000", "receive:i32:23", "i32:9"],
        "return 17\nstate 03000000\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn switches_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn switches_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
