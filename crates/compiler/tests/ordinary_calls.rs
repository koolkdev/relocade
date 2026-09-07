use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};
use wasm86_compiler::{
    Func, FunctionImport, Mem, MemoryImport, Program, Signature, Type, I1, I32, I64, I8,
};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, Validator};

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

fn imported(program: &mut Program, parameters: &[Type], result: Type) -> Func {
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

fn ordered_imports() -> Vec<u8> {
    let mut program = Program::new();
    let receive = imported(&mut program, &[Type::I32, Type::I64], Type::I64);
    let run = program.declare(signature(&[Type::I32, Type::I64], Type::I64));
    let mut body = program.define(run).unwrap();
    let word = body.parameter::<I32>(0).unwrap().add(1);
    let wide = body.parameter::<I64>(1).unwrap().add(1);
    let arguments = [word.argument(), wide.argument()];
    let first = body.call::<I64>(receive, &arguments).unwrap();
    let _second = body.call::<I64>(receive, &arguments).unwrap();
    body.return_(first.add(&first)).unwrap();
    compile_module(program, run)
}

fn narrow_arguments_and_result() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = imported(&mut program, &[Type::I8, Type::I8], Type::I8);
    let run = program.declare(signature(&[Type::I8], Type::I8));
    let mut body = program.define(run).unwrap();
    let raw = body.parameter::<I8>(0).unwrap().add(1);
    body.store(state, 0, &raw).unwrap();
    let answer = body
        .call::<I8>(receive, &[raw.argument(), raw.argument()])
        .unwrap();
    body.return_(answer.add(1)).unwrap();
    compile_module(program, run)
}

fn transitive_mutation() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[], Type::I32));
    let wrapper = program.declare(signature(&[], Type::I32));
    let mutator = program.declare(signature(&[], Type::I32));
    let mut body = program.define(run).unwrap();
    let before = body.load::<I32>(state, 0).unwrap();
    let answer = body.call::<I32>(wrapper, &[]).unwrap();
    body.store(state, 4, &answer).unwrap();
    let after = body.load::<I32>(state, 0).unwrap();
    body.return_(before.add(&after)).unwrap();
    program
        .define(wrapper)
        .unwrap()
        .tail_call(mutator, &[])
        .unwrap();
    let mut body = program.define(mutator).unwrap();
    body.store::<I32>(state, 0, 9).unwrap();
    body.return_(5).unwrap();
    compile_module(program, run)
}

enum ReadUse {
    Discard,
    ReturnSnapshot,
    AddFreshRead,
}

fn readonly_call(offset: u32, store_offset: u32, result_use: ReadUse) -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[], Type::I32));
    let reader = program.declare(signature(&[], Type::I32));
    let mut body = program.define(run).unwrap();
    let before = body.call::<I32>(reader, &[]).unwrap();
    if store_offset != 0 {
        body.store::<I32>(state, 0, 1).unwrap();
    }
    body.store::<I32>(state, store_offset, 9).unwrap();
    match result_use {
        ReadUse::Discard => body.return_(7).unwrap(),
        ReadUse::ReturnSnapshot => body.return_(&before).unwrap(),
        ReadUse::AddFreshRead => {
            let after = body.call::<I32>(reader, &[]).unwrap();
            body.return_(before.add(&after)).unwrap();
        }
    }
    let mut body = program.define(reader).unwrap();
    let value = body.load::<I32>(state, offset).unwrap();
    body.return_(&value).unwrap();
    compile_module(program, run)
}

fn computed_helper_read() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I32], Type::I32));
    let reader = program.declare(signature(&[Type::I32], Type::I32));
    let mut body = program.define(run).unwrap();
    let address = body.parameter::<I32>(0).unwrap();
    let before = body.call::<I32>(reader, &[address.argument()]).unwrap();
    body.store::<I32>(state, 0, 9).unwrap();
    body.return_(&before).unwrap();
    let mut body = program.define(reader).unwrap();
    let address = body.parameter::<I32>(0).unwrap();
    let loaded = body.load_at::<I32>(state, &address, 0).unwrap();
    body.return_(&loaded).unwrap();
    compile_module(program, run)
}

fn predicate_result() -> Vec<u8> {
    let mut program = Program::new();
    let run = program.declare(signature(&[Type::I32], Type::I32));
    let predicate = program.declare(signature(&[Type::I32], Type::I1));
    let mut body = program.define(run).unwrap();
    let input = body.parameter::<I32>(0).unwrap();
    let condition = body.call::<I1>(predicate, &[input.argument()]).unwrap();
    body.if_(&condition, |branch| branch.return_(11)).unwrap();
    body.return_(22).unwrap();
    let body = program.define(predicate).unwrap();
    let input = body.parameter::<I32>(0).unwrap();
    body.return_(input.eq(7)).unwrap();
    compile_module(program, run)
}

fn branch_call() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1], Type::I32));
    let helper = program.declare(signature(&[], Type::I32));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    body.store::<I32>(state, 0, 1).unwrap();
    body.if_(&condition, |mut branch| {
        let _unused = branch.call::<I32>(helper, &[])?;
        Ok(())
    })
    .unwrap();
    body.store::<I32>(state, 4, 2).unwrap();
    body.return_(17).unwrap();
    let mut body = program.define(helper).unwrap();
    body.store::<I32>(state, 8, 3).unwrap();
    let trapped = body.load::<I32>(state, 65536).unwrap();
    body.return_(&trapped).unwrap();
    compile_module(program, run)
}

fn snapshot_across_a_tail_arm() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program.declare(signature(&[Type::I1], Type::I32));
    let wrapper = program.declare(signature(&[], Type::I32));
    let mutator = program.declare(signature(&[], Type::I32));
    let mut body = program.define(run).unwrap();
    let condition = body.parameter::<I1>(0).unwrap();
    let before = body.load::<I32>(state, 65536).unwrap();
    body.store::<I32>(state, 0, 1).unwrap();
    body.if_(&condition, |branch| branch.tail_call(wrapper, &[]))
        .unwrap();
    body.return_(&before).unwrap();
    program
        .define(wrapper)
        .unwrap()
        .tail_call(mutator, &[])
        .unwrap();
    let mut body = program.define(mutator).unwrap();
    body.store::<I32>(state, 65536, 9).unwrap();
    body.return_(5).unwrap();
    compile_module(program, run)
}

fn trapping_argument() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let receive = imported(&mut program, &[Type::I32], Type::I32);
    let run = program.declare(signature(&[], Type::I32));
    let mut body = program.define(run).unwrap();
    let argument = body.load::<I32>(state, 65536).unwrap();
    body.store::<I32>(state, 0, 1).unwrap();
    let answer = body.call::<I32>(receive, &[argument.argument()]).unwrap();
    body.store::<I32>(state, 4, 2).unwrap();
    body.return_(&answer).unwrap();
    compile_module(program, run)
}

#[derive(Debug, PartialEq)]
enum Event {
    Call,
    Tail,
    Return,
    If,
    End,
    Load(u64),
    Store(u64),
}

#[derive(Default)]
struct Code {
    events: Vec<Event>,
    additions: usize,
    masks: usize,
    drops: usize,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut imports = 0;
    let mut exported = None;
    let mut index = 0;
    let mut code = Code::default();
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
                        assert_eq!(export.kind, ExternalKind::Func);
                        exported = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let current = index + imports;
                index += 1;
                if Some(current) != exported {
                    continue;
                }
                let mut depth = 0;
                let mut operators = body.get_operators_reader().unwrap();
                while !operators.eof() {
                    let event = match operators.read().unwrap() {
                        Operator::Call { .. } => Event::Call,
                        Operator::Return => Event::Return,
                        Operator::If { .. } => {
                            depth += 1;
                            Event::If
                        }
                        Operator::End if depth > 0 => {
                            depth -= 1;
                            Event::End
                        }
                        Operator::I32Load { memarg } => Event::Load(memarg.offset),
                        Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                            Event::Store(memarg.offset)
                        }
                        Operator::I32Add | Operator::I64Add => {
                            code.additions += 1;
                            continue;
                        }
                        Operator::Drop => {
                            code.drops += 1;
                            continue;
                        }
                        Operator::I32And => {
                            code.masks += 1;
                            continue;
                        }
                        Operator::ReturnCall { .. } => Event::Tail,
                        _ => continue,
                    };
                    code.events.push(event);
                }
            }
            _ => {}
        }
    }
    assert!(exported.is_some_and(|export| export >= imports && export < index + imports));
    code
}

#[test]
fn imported_calls_execute_once_each_even_without_memory_or_a_used_result() {
    let code = inspect(&ordered_imports());
    assert_eq!(code.events, [Event::Call, Event::Call, Event::Return]);
    assert_eq!(code.additions, 3);
    assert_eq!(code.drops, 1);
}

#[test]
fn calls_depending_on_recursion_are_retained_when_unused() {
    let mut program = Program::new();
    let recursive = program.declare(signature(&[], Type::I32));
    let wrapper = program.declare(signature(&[], Type::I32));
    let run = program.declare(signature(&[], Type::I32));
    for (function, target) in [(recursive, recursive), (wrapper, recursive), (run, wrapper)] {
        let mut body = program.define(function).unwrap();
        let _unused = body.call::<I32>(target, &[]).unwrap();
        body.return_(7).unwrap();
    }
    let code = inspect(&compile_module(program, run));
    assert_eq!(code.events, [Event::Call, Event::Return]);
    assert_eq!(code.drops, 1);
}

#[test]
fn repeated_narrow_arguments_share_normalization_and_results_are_canonical() {
    let code = inspect(&narrow_arguments_and_result());
    assert_eq!(code.events, [Event::Store(0), Event::Call, Event::Return]);
    assert_eq!((code.additions, code.masks), (2, 2));
}

#[test]
fn transitive_callee_writes_preserve_earlier_loads() {
    let code = inspect(&transitive_mutation());
    assert_eq!(
        code.events,
        [
            Event::Load(0),
            Event::Call,
            Event::Store(4),
            Event::Load(0),
            Event::Return
        ]
    );
}

#[test]
fn readonly_calls_follow_result_demand_without_crossing_aliasing_writes() {
    assert_eq!(
        inspect(&computed_helper_read()).events,
        [Event::Call, Event::Store(0), Event::Return]
    );
    assert_eq!(
        inspect(&readonly_call(0, 0, ReadUse::ReturnSnapshot)).events,
        [Event::Call, Event::Store(0), Event::Return]
    );
    assert_eq!(
        inspect(&readonly_call(8, 4, ReadUse::ReturnSnapshot)).events,
        [Event::Store(0), Event::Store(4), Event::Call, Event::Return]
    );
    assert_eq!(
        inspect(&readonly_call(65536, 0, ReadUse::Discard)).events,
        [Event::Store(0), Event::Return]
    );
    assert_eq!(
        inspect(&readonly_call(0, 0, ReadUse::AddFreshRead)).events,
        [Event::Call, Event::Store(0), Event::Call, Event::Return]
    );
}

#[test]
fn traps_in_calls_and_arguments_respect_their_control_path() {
    assert_eq!(
        inspect(&branch_call()).events,
        [
            Event::Store(0),
            Event::If,
            Event::Call,
            Event::End,
            Event::Store(4),
            Event::Return
        ]
    );
    assert_eq!(
        inspect(&trapping_argument()).events,
        [
            Event::Store(0),
            Event::Load(65536),
            Event::Call,
            Event::Store(4),
            Event::Return
        ]
    );
    assert_eq!(
        inspect(&readonly_call(65536, 0, ReadUse::ReturnSnapshot)).events,
        [Event::Store(0), Event::Call, Event::Return]
    );
    assert_eq!(
        inspect(&readonly_call(65536, 65536, ReadUse::ReturnSnapshot)).events,
        [
            Event::Call,
            Event::Store(0),
            Event::Store(65536),
            Event::Return
        ]
    );
}

#[test]
fn callee_writes_in_a_returning_arm_preserve_earlier_snapshots() {
    assert_eq!(
        inspect(&snapshot_across_a_tail_arm()).events,
        [
            Event::Load(65536),
            Event::Store(0),
            Event::If,
            Event::Tail,
            Event::End,
            Event::Return
        ]
    );
}

#[test]
fn a_logical_one_bit_call_result_can_control_a_branch() {
    let code = inspect(&predicate_result());
    assert_eq!(
        code.events,
        [
            Event::Call,
            Event::If,
            Event::Return,
            Event::End,
            Event::Return
        ]
    );
    assert_eq!(code.masks, 0);
}

struct ModuleFile(PathBuf);

impl ModuleFile {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "wasm86-calls-{}-{}.wasm",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }

    fn check(&self, flags: &[&str], inputs: &[&str], expected: &str) {
        let output = Command::new("node")
            .args(flags)
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/support/execute-tail.mjs"
            ))
            .arg(&self.0)
            .arg("run")
            .args(inputs)
            .output()
            .expect("the explicit V8 lane requires Node.js on PATH");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected,
            "inputs {inputs:?}, flags {flags:?}"
        );
    }
}

impl Drop for ModuleFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn check_execution(flags: &[&str]) {
    ModuleFile::new(&ordered_imports()).check(
        flags,
        &[
            "-",
            "receive:i64:9223372036854775807",
            "i32:2147483647",
            "i64:9223372036854775807",
        ],
        concat!(
            "receive(-2147483648,-9223372036854775808)\n",
            "receive(-2147483648,-9223372036854775808)\n",
            "return -2\n"
        ),
    );
    ModuleFile::new(&narrow_arguments_and_result()).check(
        flags,
        &["a55a", "receive:i32:255", "i32:255"],
        "receive(0,0) 005a\nreturn 0\nstate 005a\n",
    );
    ModuleFile::new(&transitive_mutation()).check(
        flags,
        &["070000000b000000a55a", ""],
        "return 16\nstate 0900000005000000a55a\n",
    );
    ModuleFile::new(&readonly_call(0, 0, ReadUse::ReturnSnapshot)).check(
        flags,
        &["070000000b000000a55a", ""],
        "return 7\nstate 090000000b000000a55a\n",
    );
    ModuleFile::new(&readonly_call(8, 4, ReadUse::ReturnSnapshot)).check(
        flags,
        &["070000000b00000005000000a55a", ""],
        "return 5\nstate 010000000900000005000000a55a\n",
    );
    ModuleFile::new(&readonly_call(0, 0, ReadUse::AddFreshRead)).check(
        flags,
        &["070000000b000000a55a", ""],
        "return 16\nstate 090000000b000000a55a\n",
    );
    ModuleFile::new(&readonly_call(65536, 0, ReadUse::Discard)).check(
        flags,
        &["07000000a55a", ""],
        "return 7\nstate 09000000a55a\n",
    );
    ModuleFile::new(&readonly_call(65536, 0, ReadUse::ReturnSnapshot)).check(
        flags,
        &["07000000a55a", ""],
        "return trap\nstate 09000000a55a\n",
    );
    ModuleFile::new(&readonly_call(65536, 65536, ReadUse::ReturnSnapshot)).check(
        flags,
        &["07000000a55a", ""],
        "return trap\nstate 07000000a55a\n",
    );
    let computed = ModuleFile::new(&computed_helper_read());
    computed.check(
        flags,
        &["070000000b00000005000000a55a", "", "i32:8"],
        "return 5\nstate 090000000b00000005000000a55a\n",
    );
    // A computed helper read conservatively aliases every byte in its memory.
    computed.check(
        flags,
        &["07000000a55a", "", "i32:65536"],
        "return trap\nstate 07000000a55a\n",
    );
    let predicate = ModuleFile::new(&predicate_result());
    predicate.check(flags, &["-", "", "i32:7"], "return 11\n");
    predicate.check(flags, &["-", "", "i32:5"], "return 22\n");
    let branch = ModuleFile::new(&branch_call());
    branch.check(
        flags,
        &["07000000050000000b000000a55a", "", "i32:0"],
        "return 17\nstate 01000000020000000b000000a55a\n",
    );
    branch.check(
        flags,
        &["07000000050000000b000000a55a", "", "i32:1"],
        "return trap\nstate 010000000500000003000000a55a\n",
    );
    // A possible aliasing write in the returning arm preserves the authored snapshot.
    let tail_arm = ModuleFile::new(&snapshot_across_a_tail_arm());
    tail_arm.check(
        flags,
        &["07000000a55a", "", "i32:0"],
        "return trap\nstate 07000000a55a\n",
    );
    tail_arm.check(
        flags,
        &["07000000a55a", "", "i32:1"],
        "return trap\nstate 07000000a55a\n",
    );
    ModuleFile::new(&trapping_argument()).check(
        flags,
        &["0700000005000000a55a", "receive:i32:17"],
        "return trap\nstate 0100000005000000a55a\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn ordinary_calls_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn ordinary_calls_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
