use super::{compile, memory, signature, ModuleFile};
use wasm86_compiler::{
    Argument, BuildError, Func, FunctionBuilder, FunctionImport, Program, Type, I32,
};
use wasmparser::{Operator, Parser, Payload, TypeRef};

fn finish(body: FunctionBuilder<'_>, result: Option<Type>) -> Result<(), BuildError> {
    match result {
        Some(Type::I32) => body.return_(7),
        None => body.return_void(),
        _ => unreachable!("the fixtures compare I32 and absent results"),
    }
}

fn call_unused(
    body: &mut FunctionBuilder<'_>,
    target: Func,
    result: Option<Type>,
    arguments: &[Argument],
) -> Result<(), BuildError> {
    match result {
        Some(Type::I32) => {
            let _unused = body.call::<I32>(target, arguments)?;
            Ok(())
        }
        None => body.call_void(target, arguments),
        _ => unreachable!("the fixtures compare I32 and absent results"),
    }
}

#[derive(Clone, Copy, Debug)]
enum Pure {
    Empty,
    Trap,
    ReadTrap,
    TrappingArgument,
    VoidDescendant,
}

const PURE_CASES: [Pure; 5] = [
    Pure::Empty,
    Pure::Trap,
    Pure::ReadTrap,
    Pure::TrappingArgument,
    Pure::VoidDescendant,
];

fn pure_call(result: Option<Type>, behavior: Pure) -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let helper = program
        .function(signature(&[Type::I32], result), |mut body| {
            match behavior {
                Pure::Trap => return body.trap(),
                Pure::ReadTrap => {
                    let invalid = body.load::<I32>(state, 65536)?;
                    body.if_(invalid.eq(0), |arm| arm.trap())?;
                }
                Pure::VoidDescendant => {
                    let trap = body
                        .program()
                        .function(signature(&[], None), |body| body.trap())?;
                    body.call_void(trap, &[])?;
                }
                Pure::Empty | Pure::TrappingArgument => {}
            }
            finish(body, result)
        })
        .unwrap();
    program.export("helper", helper).unwrap();
    let run = program
        .function(signature(&[], None), |mut body| {
            body.store::<I32>(state, 0, 1)?;
            let argument = match behavior {
                Pure::TrappingArgument => body.load::<I32>(state, 65536)?.into(),
                _ => 7.into(),
            };
            call_unused(&mut body, helper, result, &[argument])?;
            body.store::<I32>(state, 0, 2)?;
            body.return_void()
        })
        .unwrap();
    compile(program, run)
}

#[derive(Clone, Copy, Debug)]
enum Effect {
    Write,
    WriteThenTrap,
    TrappingArgument,
    Host,
    Recursive,
}

const EFFECT_CASES: [Effect; 5] = [
    Effect::Write,
    Effect::WriteThenTrap,
    Effect::TrappingArgument,
    Effect::Host,
    Effect::Recursive,
];

fn effectful_call(result: Option<Type>, behavior: Effect) -> Vec<u8> {
    let mut program = Program::new();
    let state = memory(&mut program);
    let helper = if matches!(behavior, Effect::Host) {
        program.import_function(FunctionImport {
            module: "test".into(),
            name: "receive".into(),
            signature: signature(&[Type::I32], result),
        })
    } else {
        let helper = program.declare(signature(&[Type::I32], result));
        let mut body = program.define(helper).unwrap();
        let input = body.parameter::<I32>(0).unwrap();
        if matches!(behavior, Effect::Recursive) {
            body.if_(input.eq(0), |arm| finish(arm, result)).unwrap();
            body.tail_call(helper, &[input.sub(1).into()]).unwrap();
        } else {
            body.store(state, 4, input).unwrap();
            if matches!(behavior, Effect::WriteThenTrap) {
                body.trap().unwrap();
            } else {
                finish(body, result).unwrap();
            }
        }
        helper
    };
    let wrapper = program
        .function(signature(&[Type::I32], result), |mut body| {
            let input = body.parameter::<I32>(0)?;
            call_unused(&mut body, helper, result, &[input.into()])?;
            finish(body, result)
        })
        .unwrap();
    let run = program
        .function(signature(&[], None), |mut body| {
            body.store::<I32>(state, 0, 1)?;
            let argument = match behavior {
                Effect::TrappingArgument => body.load::<I32>(state, 65536)?.into(),
                _ => 7.into(),
            };
            call_unused(&mut body, wrapper, result, &[argument])?;
            body.store::<I32>(state, 0, 2)?;
            body.return_void()
        })
        .unwrap();
    compile(program, run)
}

#[derive(Debug, Default, PartialEq)]
struct Code {
    calls: usize,
    loads: usize,
    stores: usize,
}

fn run_code(bytes: &[u8]) -> Code {
    let mut function_index = 0;
    let mut exported = None;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ImportSection(imports) => {
                for import in imports {
                    if matches!(import.unwrap().ty, TypeRef::Func(_)) {
                        function_index += 1;
                    }
                }
            }
            Payload::ExportSection(exports) => {
                for export in exports {
                    let export = export.unwrap();
                    if export.name == "run" {
                        exported = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                if Some(function_index) == exported {
                    let mut code = Code::default();
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::Call { .. } => code.calls += 1,
                            Operator::I32Load { .. } => code.loads += 1,
                            Operator::I32Store { .. } => code.stores += 1,
                            _ => {}
                        }
                    }
                    return code;
                }
                function_index += 1;
            }
            _ => {}
        }
    }
    panic!("the fixture exports a defined run function")
}

#[test]
fn result_presence_does_not_retain_calls_without_writes_or_unknown_effects() {
    for result in [None, Some(Type::I32)] {
        for behavior in PURE_CASES {
            assert_eq!(
                run_code(&pure_call(result, behavior)),
                Code {
                    calls: 0,
                    loads: 0,
                    stores: 2,
                },
                "{result:?}, {behavior:?}",
            );
        }
    }
}

#[test]
fn result_presence_does_not_hide_transitive_writes_hosts_or_recursion() {
    for result in [None, Some(Type::I32)] {
        for behavior in EFFECT_CASES {
            assert_eq!(
                run_code(&effectful_call(result, behavior)),
                Code {
                    calls: 1,
                    loads: usize::from(matches!(behavior, Effect::TrappingArgument)),
                    stores: 2,
                },
                "{result:?}, {behavior:?}",
            );
        }
    }
}

pub(super) fn check_execution(flags: &[&str]) {
    for result in [None, Some(Type::I32)] {
        for behavior in PURE_CASES {
            let module = ModuleFile::new(&pure_call(result, behavior));
            module.check(
                flags,
                "execute-memory.mjs",
                &["state:07000000"],
                "undefined\nstate:02000000\n",
            );
            if matches!(behavior, Pure::Trap | Pure::ReadTrap) {
                module.check(
                    flags,
                    "execute-tail.mjs",
                    &["helper", "07000000", "", "i32:7"],
                    "return trap\nstate 07000000\n",
                );
            }
        }
        for behavior in EFFECT_CASES {
            let expected = match behavior {
                Effect::Write => "return undefined\nstate 0200000007000000\n",
                Effect::WriteThenTrap => "return trap\nstate 0100000007000000\n",
                Effect::TrappingArgument => "return trap\nstate 0100000000000000\n",
                Effect::Host => {
                    "receive(7) 0100000000000000\nreturn undefined\nstate 0200000000000000\n"
                }
                Effect::Recursive => "return undefined\nstate 0200000000000000\n",
            };
            ModuleFile::new(&effectful_call(result, behavior)).check(
                flags,
                "execute-tail.mjs",
                &["run", "0700000000000000", "receive:i32:99"],
                expected,
            );
        }
    }
}
