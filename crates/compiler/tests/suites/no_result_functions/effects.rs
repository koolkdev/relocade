use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, MemoryBytes, TestModule, Value};
use wasm86_compiler::{Argument, BuildError, Func, FunctionBuilder, Type, I32};
use wasmparser::{Operator, Parser, Payload, TypeRef, Validator};

fn finish(body: FunctionBuilder<'_>, result: Option<Type>) -> Result<(), BuildError> {
    match result {
        Some(Type::I32) => body.return_(7),
        None => body.return_(()),
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
        None => body.call::<()>(target, arguments),
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

fn pure_call(result: Option<Type>, behavior: Pure) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let helper = fixture
        .program
        .function(signature(&[Type::I32], result.as_slice()), |mut body| {
            match behavior {
                Pure::Trap => return body.trap(),
                Pure::ReadTrap => {
                    let invalid = body.load::<I32>(state, 65536)?;
                    body.if_(invalid.eq(0), |arm| arm.trap())?;
                }
                Pure::VoidDescendant => {
                    let trap = body
                        .program()
                        .function(signature(&[], &[]), |body| body.trap())?;
                    body.call::<()>(trap, &[])?;
                }
                Pure::Empty | Pure::TrappingArgument => {}
            }
            finish(body, result)
        })
        .unwrap();
    fixture.program.export("helper", helper).unwrap();
    fixture.function(&[], &[], |mut body| {
        body.store::<I32>(state, 0, 1)?;
        let argument = match behavior {
            Pure::TrappingArgument => body.load::<I32>(state, 65536)?.into(),
            _ => 7.into(),
        };
        call_unused(&mut body, helper, result, &[argument])?;
        body.store::<I32>(state, 0, 2)?;
        body.return_(())
    })
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

fn effectful_call(result: Option<Type>, behavior: Effect) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0, 0, 0, 0, 0]);
    let helper = if matches!(behavior, Effect::Host) {
        fixture.callback(
            "receive",
            signature(&[Type::I32], result.as_slice()),
            result.map(|_| Value::I32(99)).as_slice(),
        )
    } else {
        let helper = fixture
            .program
            .declare(signature(&[Type::I32], result.as_slice()));
        let mut body = fixture.program.define(helper).unwrap();
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
    let wrapper = fixture
        .program
        .function(signature(&[Type::I32], result.as_slice()), |mut body| {
            let input = body.parameter::<I32>(0)?;
            call_unused(&mut body, helper, result, &[input.into()])?;
            finish(body, result)
        })
        .unwrap();
    fixture.function(&[], &[], |mut body| {
        body.store::<I32>(state, 0, 1)?;
        let argument = match behavior {
            Effect::TrappingArgument => body.load::<I32>(state, 65536)?.into(),
            _ => 7.into(),
        };
        call_unused(&mut body, wrapper, result, &[argument])?;
        body.store::<I32>(state, 0, 2)?;
        body.return_(())
    })
}

#[derive(Debug, Default, PartialEq)]
struct Code {
    calls: usize,
    loads: usize,
    stores: usize,
}

fn run_code(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
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
                run_code(pure_call(result, behavior).bytes()),
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
                run_code(effectful_call(result, behavior).bytes()),
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

#[test]
fn unused_pure_calls_skip_their_body_for_both_result_forms() {
    for result in [None, Some(Type::I32)] {
        for behavior in PURE_CASES {
            let module = pure_call(result, behavior);
            let mut instance = module.instantiate();
            instance.call::<()>(()).unwrap();
            assert_eq!(&instance.memory("state")[..4], &[2, 0, 0, 0]);

            if matches!(behavior, Pure::Trap | Pure::ReadTrap) {
                let mut instance = module.instantiate();
                if result.is_some() {
                    assert!(instance.call_export::<i32>("helper", 7).is_err());
                } else {
                    assert!(instance.call_export::<()>("helper", 7).is_err());
                }
                assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
            }
        }
    }
}

#[test]
fn unused_effectful_calls_keep_ordered_effects_for_both_result_forms() {
    for result in [None, Some(Type::I32)] {
        for behavior in EFFECT_CASES {
            let mut instance = effectful_call(result, behavior).instantiate();
            let returned = instance.call::<()>(());
            if matches!(behavior, Effect::WriteThenTrap | Effect::TrappingArgument) {
                assert!(returned.is_err(), "{result:?}, {behavior:?}");
            } else {
                returned.unwrap();
            }
            let expected_memory = match behavior {
                Effect::Write => [2, 0, 0, 0, 7, 0, 0, 0],
                Effect::WriteThenTrap => [1, 0, 0, 0, 7, 0, 0, 0],
                Effect::TrappingArgument => [1, 0, 0, 0, 0, 0, 0, 0],
                Effect::Host | Effect::Recursive => [2, 0, 0, 0, 0, 0, 0, 0],
            };
            assert_eq!(
                &instance.memory("state")[..8],
                &expected_memory,
                "{result:?}, {behavior:?}"
            );
            if matches!(behavior, Effect::Host) {
                assert_eq!(
                    instance.callbacks(),
                    &[Call::new("receive", &[Value::I32(7)])
                        .with_memories(&[MemoryBytes::new("state", &[1, 0, 0, 0, 0, 0, 0, 0])])]
                );
            } else {
                assert!(instance.callbacks().is_empty());
            }
        }
    }
}
