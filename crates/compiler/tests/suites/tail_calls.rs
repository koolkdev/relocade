use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, Callback, Input, MemoryBytes, Observation, TestModule, Value};

use wasm86_compiler::{
    BuildError, FunctionImport, Program, Signature, Type, I1, I16, I32, I64, I8,
};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

fn zero_arguments() -> TestModule {
    let mut fixture = Fixture::new();
    let target = fixture.callback(
        "receive",
        signature(&[], &[Type::I64]),
        &[Value::I64(9223372036854775807)],
    );
    fixture.function(&[], &[Type::I64], |body| body.tail_call(target, &[]))
}

fn shared_arguments(store: bool, second_root: bool, callback_result: i64) -> TestModule {
    let mut fixture = Fixture::new();
    let state = store.then(|| fixture.memory("state", &[0xff, 0xa5, 0x5a]));
    let parameters = if second_root {
        vec![Type::I8; 3]
    } else {
        vec![Type::I8; 2]
    };
    let target = fixture.callback(
        "receive",
        signature(&parameters, &[Type::I64]),
        &[Value::I64(callback_result)],
    );
    fixture.function(&[Type::I8], &[Type::I64], |mut body| {
        let raw = body.parameter::<I8>(0)?.add(1);
        let raw = body.value(&raw)?;
        let other = raw.add(1);
        if let Some(state) = state {
            body.store(state, 0, &raw)?;
        }
        let mut arguments = vec![raw.argument()];
        if second_root {
            arguments.push(other.argument());
        }
        arguments.push(raw.argument());
        body.tail_call(target, &arguments)
    })
}

fn canonical_arguments() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xff, 0xa5, 0x5a]);
    let target = fixture.callback(
        "receive",
        signature(&[Type::I8, Type::I16, Type::I1], &[Type::I8]),
        &[Value::I32(255)],
    );
    fixture.function(&[Type::I16], &[Type::I8], |mut body| {
        let loaded = body.load::<I8>(state, 0)?;
        let parameter = body.parameter::<I16>(0)?;
        body.tail_call(
            target,
            &[loaded.argument(), parameter.argument(), true.into()],
        )
    })
}

fn imported_and_defined_targets() -> TestModule {
    let mut fixture = Fixture::new();
    fixture.callback("unused", signature(&[], &[Type::I1]), &[Value::I32(0)]);
    let run = fixture.program.declare(signature(&[], &[Type::I64]));
    let right = fixture.callback(
        "right",
        signature(&[Type::I32], &[Type::I64]),
        &[Value::I64(101)],
    );
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let helper = fixture
        .program
        .declare(signature(&[Type::I32], &[Type::I64]));
    let left = fixture.callback(
        "left",
        signature(&[Type::I32], &[Type::I64]),
        &[Value::I64(202)],
    );
    let direct = fixture.callback(
        "direct",
        signature(&[Type::I64], &[Type::I64]),
        &[Value::I64(-9223372036854775808)],
    );
    let other = fixture.program.declare(signature(&[], &[Type::I64]));
    let mut body = fixture.program.define(run).unwrap();
    body.store::<I32>(state, 0, 11).unwrap();
    body.tail_call(helper, &[11.into()]).unwrap();
    let body = fixture.program.define(helper).unwrap();
    let parameter = body.parameter::<I32>(0).unwrap();
    body.tail_call(left, &[parameter.argument()]).unwrap();
    let body = fixture.program.define(other).unwrap();
    body.tail_call(right, &[22.into()]).unwrap();
    for (name, function) in [
        ("run", run),
        ("helper", helper),
        ("other", other),
        ("direct", direct),
    ] {
        fixture.program.export(name, function).unwrap();
    }
    fixture.compile()
}

#[derive(Default)]
struct Code {
    locals: u32,
    writes: usize,
    adds: usize,
    masks: usize,
    tails: Vec<u32>,
    types: Vec<(Vec<ValType>, Vec<ValType>)>,
    imports: Vec<(String, TypeRef)>,
    functions: Vec<u32>,
    exports: Vec<(String, u32)>,
    store_memories: Vec<u32>,
}

fn inspect(bytes: &[u8]) -> Code {
    Validator::new().validate_all(bytes).unwrap();
    let mut code = Code::default();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::TypeSection(types) => {
                for ty in types.into_iter_err_on_gc_types() {
                    let ty = ty.unwrap();
                    code.types
                        .push((ty.params().to_vec(), ty.results().to_vec()));
                }
            }
            Payload::ImportSection(imports) => {
                for import in imports {
                    let import = import.unwrap();
                    code.imports.push((import.name.into(), import.ty));
                }
            }
            Payload::FunctionSection(functions) => code
                .functions
                .extend(functions.into_iter().map(Result::unwrap)),
            Payload::ExportSection(exports) => {
                for export in exports {
                    let export = export.unwrap();
                    assert_eq!(export.kind, ExternalKind::Func);
                    code.exports.push((export.name.into(), export.index));
                }
            }
            Payload::CodeSectionEntry(body) => {
                for local in body.get_locals_reader().unwrap() {
                    code.locals += local.unwrap().0;
                }
                let mut operators = body.get_operators_reader().unwrap();
                while !operators.eof() {
                    match operators.read().unwrap() {
                        Operator::I32Add | Operator::I64Add => code.adds += 1,
                        Operator::I32And => code.masks += 1,
                        Operator::LocalSet { .. } | Operator::LocalTee { .. } => code.writes += 1,
                        Operator::ReturnCall { function_index } => code.tails.push(function_index),
                        Operator::I32Store { memarg } | Operator::I32Store8 { memarg } => {
                            code.store_memories.push(memarg.memory)
                        }
                        Operator::Call { .. } | Operator::Return => {
                            panic!("a tail call must use return_call")
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    code
}

#[test]
fn a_tail_call_can_have_no_arguments() {
    let code = inspect(zero_arguments().bytes());
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (0, 0, 0, 0)
    );
    assert_eq!(code.tails.len(), 1);
}

#[test]
fn duplicate_narrow_arguments_share_arithmetic_and_normalization() {
    let code = inspect(shared_arguments(false, false, 0).bytes());
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (1, 1, 1, 1)
    );
    assert_eq!(code.tails.len(), 1);
}

#[test]
fn stores_share_raw_values_with_normalized_arguments() {
    let code = inspect(shared_arguments(true, false, 0).bytes());
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (1, 1, 1, 2)
    );
    let code = inspect(shared_arguments(true, true, 0).bytes());
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (2, 2, 2, 2)
    );
}

#[test]
fn canonical_arguments_need_no_masks() {
    let code = inspect(canonical_arguments().bytes());
    assert_eq!(
        (code.adds, code.masks, code.locals, code.writes),
        (0, 0, 0, 0)
    );
    assert_eq!(code.tails.len(), 1);
}

#[test]
fn function_imports_remap_calls_and_exports_without_shifting_memories() {
    let code = inspect(imported_and_defined_targets().bytes());
    let imports: Vec<_> = code
        .imports
        .iter()
        .filter_map(|(name, ty)| match ty {
            TypeRef::Func(index) => Some((name.as_str(), *index)),
            _ => None,
        })
        .collect();
    assert_eq!(
        imports.iter().map(|(name, _)| *name).collect::<Vec<_>>(),
        ["right", "left", "direct"]
    );
    assert_eq!(
        code.types,
        [
            (vec![], vec![ValType::I64]),
            (vec![ValType::I32], vec![ValType::I64]),
            (vec![ValType::I64], vec![ValType::I64])
        ]
    );
    assert_eq!(code.functions, [0, 1, 0]);
    assert_eq!(imports[0].1, code.functions[1]);
    assert_eq!(imports[1].1, code.functions[1]);
    assert_eq!(imports[2].1, 2);
    let index = |name| {
        code.exports
            .iter()
            .find(|(export, _)| export == name)
            .unwrap()
            .1
    };
    assert_eq!(code.tails, [index("helper"), 1, 0]);
    assert_eq!(index("direct"), 2);
    assert_eq!((index("run"), index("other")), (3, 5));
    assert_eq!(code.store_memories, [0]);
}

#[test]
fn tail_signatures_require_logical_argument_and_result_types() {
    for (target_parameter, target_result) in [(Type::I8, Type::I1), (Type::I1, Type::I8)] {
        let mut program = Program::new();
        let target = program.import_function(FunctionImport {
            module: "test".into(),
            name: "receive".into(),
            signature: Signature {
                parameters: vec![target_parameter],
                results: vec![target_result],
            },
        });
        let run = program.declare(Signature {
            parameters: vec![],
            results: vec![Type::I1],
        });
        let body = program.define(run).unwrap();
        let bit = body.value::<I1>(true).unwrap();
        assert!(matches!(
            body.tail_call(target, &[bit.argument()]),
            Err(BuildError::TypeMismatch { .. })
        ));
        let body = program.define(run).unwrap();
        body.return_(false).unwrap();
        let bytes = program.compile().unwrap();
        assert!(Parser::new(0)
            .parse_all(&bytes)
            .all(|part| !matches!(part.unwrap(), Payload::ImportSection(_))));
    }
}

#[test]
fn tail_calls_accept_zero_arguments_at_runtime() {
    let mut instance = zero_arguments().instantiate();
    assert_eq!(instance.call::<i64>(()), Ok(9223372036854775807));
    assert_eq!(instance.callbacks(), &[Call::new("receive", &[])]);
}

#[test]
fn tail_calls_reuse_narrow_argument_values_at_runtime() {
    let mut instance = shared_arguments(false, false, 1234567890123456789).instantiate();
    assert_eq!(instance.call::<i64>(255), Ok(1234567890123456789));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(0), Value::I32(0)])]
    );
}

#[test]
fn tail_calls_share_narrow_arguments_with_stores_at_runtime() {
    let mut instance = shared_arguments(true, false, -9223372036854775808).instantiate();
    assert_eq!(instance.call::<i64>(255), Ok(-9223372036854775808));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("receive", &[Value::I32(0), Value::I32(0)])
            .with_memories(&[MemoryBytes::new("state", &[0, 0xa5, 0x5a])])]
    );
    assert_eq!(&instance.memory("state")[..3], &[0, 0xa5, 0x5a]);
    let mut instance = shared_arguments(true, true, 17).instantiate();
    assert_eq!(instance.call::<i64>(255), Ok(17));
    assert_eq!(
        instance.callbacks(),
        &[
            Call::new("receive", &[Value::I32(0), Value::I32(1), Value::I32(0)])
                .with_memories(&[MemoryBytes::new("state", &[0, 0xa5, 0x5a])])
        ]
    );
    assert_eq!(&instance.memory("state")[..3], &[0, 0xa5, 0x5a]);
}

#[test]
fn tail_calls_preserve_mixed_integer_carriers_at_runtime() {
    for (arguments, callback_result, expected_arguments) in [
        (
            (1, 255, 65535, i32::MAX, i64::MAX),
            41,
            [
                Value::I32(0),
                Value::I32(0),
                Value::I64(i64::MIN),
                Value::I32(0),
                Value::I32(i32::MIN),
            ],
        ),
        (
            (0, 7, 9, 19, 41_i64),
            -1,
            [
                Value::I32(10),
                Value::I32(1),
                Value::I64(42),
                Value::I32(8),
                Value::I32(20),
            ],
        ),
    ] {
        let mut fixture = Fixture::new();
        let target = fixture.callback(
            "receive",
            signature(
                &[Type::I16, Type::I1, Type::I64, Type::I8, Type::I32],
                &[Type::I64],
            ),
            &[Value::I64(callback_result)],
        );
        let module = fixture.function(
            &[Type::I1, Type::I8, Type::I16, Type::I32, Type::I64],
            &[Type::I64],
            |body| {
                let bit = body.parameter::<I1>(0)?.add(1);
                let byte = body.parameter::<I8>(1)?.add(1);
                let half = body.parameter::<I16>(2)?.add(1);
                let word = body.parameter::<I32>(3)?.add(1);
                let wide = body.parameter::<I64>(4)?.add(1);
                body.tail_call(
                    target,
                    &[
                        half.argument(),
                        bit.argument(),
                        wide.argument(),
                        byte.argument(),
                        word.argument(),
                    ],
                )
            },
        );

        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i64>(arguments), Ok(callback_result));
        assert_eq!(
            instance.callbacks(),
            &[Call::new("receive", &expected_arguments)]
        );
    }
}

#[test]
fn tail_calls_preserve_canonical_arguments_at_runtime() {
    let mut instance = canonical_arguments().instantiate();
    assert_eq!(instance.call::<i32>(65535), Ok(255));
    assert_eq!(
        instance.callbacks(),
        &[Call::new(
            "receive",
            &[Value::I32(255), Value::I32(65535), Value::I32(1)]
        )
        .with_memories(&[MemoryBytes::new("state", &[0xff, 0xa5, 0x5a])])]
    );
    assert_eq!(&instance.memory("state")[..3], &[0xff, 0xa5, 0x5a]);
}

#[test]
fn tail_argument_traps_preserve_prior_stores_at_runtime() {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[7, 0, 0, 0]);
    let target = fixture.callback(
        "receive",
        signature(&[Type::I32], &[Type::I64]),
        &[Value::I64(99)],
    );
    let module = fixture.function(&[], &[Type::I64], |mut body| {
        let loaded = body.load::<I32>(state, 65536)?;
        body.store::<I32>(state, 0, 9)?;
        body.tail_call(target, &[loaded.argument()])
    });
    let mut instance = module.instantiate();
    assert!(instance.call::<i64>(()).is_err());
    assert!(instance.callbacks().is_empty());
    assert_eq!(&instance.memory("state")[..4], &[9, 0, 0, 0]);
}

#[test]
fn imported_and_defined_tail_targets_use_their_own_bindings_at_runtime() {
    let bindings = imported_and_defined_targets();
    let mut instance = bindings.instantiate();
    assert_eq!(instance.call::<i64>(()), Ok(202));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("left", &[Value::I32(11)])
            .with_memories(&[MemoryBytes::new("state", &[0x0b, 0, 0, 0])])]
    );
    assert_eq!(&instance.memory("state")[..4], &[0x0b, 0, 0, 0]);
    let mut instance = bindings.instantiate();
    assert_eq!(instance.call_export::<i64>("other", ()), Ok(101));
    assert_eq!(
        instance.callbacks(),
        &[Call::new("right", &[Value::I32(22)])
            .with_memories(&[MemoryBytes::new("state", &[7, 0, 0, 0])])]
    );
    assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
    let mut instance = bindings.instantiate();
    assert_eq!(
        instance.call_export::<i64>("direct", i64::MAX),
        Ok(-9223372036854775808)
    );
    assert_eq!(
        instance.callbacks(),
        &[Call::new("direct", &[Value::I64(9223372036854775807)])
            .with_memories(&[MemoryBytes::new("state", &[7, 0, 0, 0])])]
    );
    assert_eq!(&instance.memory("state")[..4], &[7, 0, 0, 0]);
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 smoke tests"]
fn v8_tail_calls_publish_memory_and_normalize_shared_arguments() {
    let module = shared_arguments(true, true, 17);
    let input = Input::call("run", &[Value::I32(255)])
        .with_memories(&[MemoryBytes::new("state", &[0xff, 0xa5, 0x5a])])
        .with_callbacks(&[Callback::new("receive", &[Value::I64(17)])]);
    assert_eq!(
        module.run_v8(&input),
        Observation::returned(&[Value::I64(17)])
            .with_callbacks(&[
                Call::new("receive", &[Value::I32(0), Value::I32(1), Value::I32(0)])
                    .with_memories(&[MemoryBytes::new("state", &[0, 0xa5, 0x5a])]),
            ])
            .with_memories(&[MemoryBytes::new("state", &[0, 0xa5, 0x5a])]),
    );
}
