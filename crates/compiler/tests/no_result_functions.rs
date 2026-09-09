#[path = "support/wasm.rs"]
mod wasm;
use wasm::ModuleFile;

#[path = "no_result_functions/effects.rs"]
mod effects;
#[path = "no_result_functions/validation.rs"]
mod validation;

use wasm86_compiler::{
    Func, FunctionImport, Mem, MemoryImport, Program, Signature, Type, I1, I32, I8,
};
use wasmparser::{Operator, Parser, Payload, Validator};

fn signature(parameters: &[Type], result: Option<Type>) -> Signature {
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

fn compile(mut program: Program, run: Func) -> Vec<u8> {
    program.export("run", run).unwrap();
    let bytes = program.compile().unwrap();
    Validator::new().validate_all(&bytes).unwrap();
    bytes
}

fn shared_writer() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let writer = program
        .function(signature(&[Type::I32], None), |mut body| {
            let value = body.parameter::<I32>(0)?;
            body.store(state, 0, value)?;
            body.return_void()
        })
        .unwrap();
    let run = program
        .function(signature(&[Type::I32], None), |mut body| {
            let input = body.parameter::<I32>(0)?;
            body.call_void(writer, &[input.add(1).into()])?;
            body.return_void()
        })
        .unwrap();
    compile(program, run)
}

fn imported_tail() -> Vec<u8> {
    let mut program = Program::new();
    let receive = program.import_function(FunctionImport {
        module: "test".into(),
        name: "receive".into(),
        signature: signature(&[Type::I8, Type::I8], None),
    });
    let relay = program
        .function(signature(&[Type::I8], None), |body| {
            let value = body.parameter::<I8>(0)?.add(1);
            body.tail_call(receive, &[(&value).into(), value.into()])
        })
        .unwrap();
    let run = program
        .function(signature(&[Type::I8], None), |mut body| {
            let value = body.parameter::<I8>(0)?.add(1);
            body.call_void(relay, &[value.into()])?;
            body.return_void()
        })
        .unwrap();
    compile(program, run)
}

fn mutating_call() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let mutator = program
        .function(signature(&[], None), |mut body| {
            body.store::<I32>(state, 0, 9)?;
            body.return_void()
        })
        .unwrap();
    let wrapper = program
        .function(signature(&[], None), |body| body.tail_call(mutator, &[]))
        .unwrap();
    let run = program
        .function(signature(&[], Some(Type::I32)), |mut body| {
            let before = body.load::<I32>(state, 0)?;
            body.call_void(wrapper, &[])?;
            let after = body.load::<I32>(state, 0)?;
            body.return_(before.add(after))
        })
        .unwrap();
    compile(program, run)
}

fn returning_value_arm(use_switch: bool) -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let run = program
        .function(signature(&[Type::I1], None), |mut body| {
            let stop = body.parameter::<I1>(0)?;
            let value = if use_switch {
                body.switch_value::<I32, _>(stop, &[1], |arm, key| match key {
                    Some(1) => arm.return_void(),
                    _ => arm.yield_(7),
                })?
            } else {
                body.if_value::<I32>(stop, |arm| arm.return_void(), |arm| arm.yield_(7))?
            };
            body.store(state, 0, value)?;
            body.return_void()
        })
        .unwrap();
    compile(program, run)
}

fn conditional_calls() -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let writer = program
        .function(signature(&[Type::I32, Type::I32], None), |mut body| {
            let first = body.parameter::<I32>(0)?;
            let second = body.parameter::<I32>(1)?;
            body.store(state, 0, first)?;
            body.store(state, 4, second)?;
            body.return_void()
        })
        .unwrap();
    let run = program
        .function(signature(&[Type::I1], None), |mut body| {
            let write = body.parameter::<I1>(0)?;
            let before = body.load::<I32>(state, 0)?;
            let shared = before.add(1);
            body.if_(write, |mut arm| {
                arm.call_void(writer, &[(&shared).into(), (&shared).into()])
            })?;
            body.store(state, 8, shared)?;
            body.return_void()
        })
        .unwrap();
    compile(program, run)
}

fn recursive_tail() -> Vec<u8> {
    let mut program = Program::new();
    let run = program.declare(signature(&[Type::I32], None));
    let mut body = program.define(run).unwrap();
    let remaining = body.parameter::<I32>(0).unwrap();
    body.if_(remaining.eq(0), |arm| arm.return_void()).unwrap();
    body.tail_call(run, &[remaining.sub(1).into()]).unwrap();
    compile(program, run)
}

#[test]
fn no_result_functions_have_empty_wasm_results_and_no_discarded_value() {
    let bytes = shared_writer();
    let mut calls = 0;
    let mut returns = 0;
    let mut bodies = 0;
    for payload in Parser::new(0).parse_all(&bytes) {
        match payload.unwrap() {
            Payload::TypeSection(types) => {
                for ty in types.into_iter_err_on_gc_types() {
                    assert!(ty.unwrap().results().is_empty());
                }
            }
            Payload::CodeSectionEntry(body) => {
                bodies += 1;
                assert_eq!(body.get_locals_reader().unwrap().get_count(), 0);
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::Call { .. } => calls += 1,
                        Operator::Return => returns += 1,
                        Operator::Drop => panic!("a no-result call has no value to discard"),
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    assert_eq!((bodies, calls, returns), (2, 1, 2));
}

#[test]
fn no_result_calls_validate_with_imports_tails_effects_and_value_arms() {
    for bytes in [
        imported_tail(),
        mutating_call(),
        returning_value_arm(false),
        returning_value_arm(true),
        conditional_calls(),
        recursive_tail(),
    ] {
        Validator::new().validate_all(&bytes).unwrap();
    }
}

fn check_execution(flags: &[&str]) {
    effects::check_execution(flags);
    ModuleFile::new(&shared_writer()).check(
        flags,
        "execute-memory.mjs",
        &["state:07000000", "--", "i32:41"],
        "undefined\nstate:2a000000\n",
    );
    ModuleFile::new(&imported_tail()).check(
        flags,
        "execute-tail.mjs",
        &["run", "-", "receive:i32:99", "i32:254"],
        "receive(0,0)\nreturn undefined\n",
    );
    ModuleFile::new(&mutating_call()).check(
        flags,
        "execute-memory.mjs",
        &["state:07000000"],
        "16\nstate:09000000\n",
    );
    for use_switch in [false, true] {
        let arms = ModuleFile::new(&returning_value_arm(use_switch));
        arms.check(
            flags,
            "execute-memory.mjs",
            &["state:01000000", "--", "i32:1"],
            "undefined\nstate:01000000\n",
        );
        arms.check(
            flags,
            "execute-memory.mjs",
            &["state:01000000", "--", "i32:0"],
            "undefined\nstate:07000000\n",
        );
    }
    let conditional = ModuleFile::new(&conditional_calls());
    conditional.check(
        flags,
        "execute-memory.mjs",
        &["state:070000000000000000000000", "--", "i32:1"],
        "undefined\nstate:080000000800000008000000\n",
    );
    conditional.check(
        flags,
        "execute-memory.mjs",
        &["state:070000000000000000000000", "--", "i32:0"],
        "undefined\nstate:070000000000000008000000\n",
    );
    ModuleFile::new(&recursive_tail()).check(
        flags,
        "execute-memory.mjs",
        &["--", "i32:300000"],
        "undefined\n\n",
    );
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn no_result_functions_execute_in_v8() {
    check_execution(&[]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn no_result_functions_execute_in_v8_optimizing() {
    check_execution(&[
        "--no-liftoff",
        "--no-wasm-lazy-compilation",
        "--no-wasm-tier-up",
    ]);
}
