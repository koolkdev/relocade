use crate::fixture::{signature, Fixture};
use crate::wasm::{TestModule, Value};
use std::collections::BTreeMap;

use wasm86_compiler::{BuildError, FunctionBuilder, Signature, Type, Val, I1, I16, I32, I64, I8};
use wasmparser::{ExternalKind, Operator, Parser, Payload, Validator};

#[derive(Clone, Copy)]
enum Form {
    If,
    IfElse,
    IfValue,
    Select,
}

impl Form {
    fn name(self) -> &'static str {
        match self {
            Self::If => "if",
            Self::IfElse => "if_else",
            Self::IfValue => "if_value",
            Self::Select => "select",
        }
    }

    fn finish(self, mut body: FunctionBuilder<'_>, condition: Val<I1>) -> Result<(), BuildError> {
        match self {
            Self::If => {
                body.if_(condition, |arm| arm.return_(17))?;
                body.return_(29)
            }
            Self::IfElse => {
                body.if_else(condition, |arm| arm.return_(17), |arm| arm.return_(29))?;
                body.trap()
            }
            Self::IfValue => {
                let value =
                    body.if_value::<I32>(condition, |arm| arm.yield_(17), |arm| arm.yield_(29))?;
                body.return_(value)
            }
            Self::Select => body.return_(condition.select::<I32>(17, 29)),
        }
    }
}

const ALL_FORMS: &[Form] = &[Form::If, Form::IfElse, Form::IfValue, Form::Select];
const VALUE_FORMS: &[Form] = &[Form::IfValue, Form::Select];

struct Predicate {
    name: &'static str,
    parameter: Type,
    build: fn(&FunctionBuilder<'_>) -> Val<I1>,
    forms: &'static [Form],
    zero_tests: (usize, usize),
    masks: usize,
    wraps: usize,
    inputs: &'static [(Value, i32)],
}

fn predicates() -> [Predicate; 8] {
    [
        Predicate {
            name: "masked",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(8).ne(0),
            forms: ALL_FORMS,
            zero_tests: (0, 0),
            masks: 1,
            wraps: 0,
            inputs: &[
                (Value::I32(0), 29),
                (Value::I32(8), 17),
                (Value::I32(16), 29),
            ],
        },
        Predicate {
            name: "nonzero",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().ne(0),
            forms: VALUE_FORMS,
            zero_tests: (0, 0),
            masks: 0,
            wraps: 0,
            inputs: &[
                (Value::I32(0), 29),
                (Value::I32(2), 17),
                (Value::I32(-2147483648), 17),
            ],
        },
        Predicate {
            name: "zero",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().and(8).eq(0),
            forms: VALUE_FORMS,
            zero_tests: (1, 0),
            masks: 1,
            wraps: 0,
            inputs: &[
                (Value::I32(0), 17),
                (Value::I32(8), 29),
                (Value::I32(16), 17),
            ],
        },
        Predicate {
            name: "byte_wrap",
            parameter: Type::I8,
            build: |body| body.parameter::<I8>(0).unwrap().add(1).ne(0),
            forms: VALUE_FORMS,
            zero_tests: (0, 0),
            masks: 1,
            wraps: 0,
            inputs: &[
                (Value::I32(0), 17),
                (Value::I32(254), 17),
                (Value::I32(255), 29),
            ],
        },
        Predicate {
            name: "word_wrap",
            parameter: Type::I16,
            build: |body| body.parameter::<I16>(0).unwrap().add(1).ne(0),
            forms: VALUE_FORMS,
            zero_tests: (0, 0),
            masks: 1,
            wraps: 0,
            inputs: &[
                (Value::I32(0), 17),
                (Value::I32(65534), 17),
                (Value::I32(65535), 29),
            ],
        },
        Predicate {
            name: "low_bit",
            parameter: Type::I32,
            build: |body| body.parameter::<I32>(0).unwrap().truncate::<I1>(),
            forms: VALUE_FORMS,
            zero_tests: (0, 0),
            masks: 1,
            wraps: 0,
            inputs: &[
                (Value::I32(0), 29),
                (Value::I32(2), 29),
                (Value::I32(3), 17),
            ],
        },
        Predicate {
            name: "wide",
            parameter: Type::I64,
            build: |body| body.parameter::<I64>(0).unwrap().ne(0u64),
            forms: VALUE_FORMS,
            zero_tests: (1, 1),
            masks: 0,
            wraps: 0,
            inputs: &[
                (Value::I64(0), 29),
                (Value::I64(4294967296), 17),
                (Value::I64(-9223372036854775808), 17),
            ],
        },
        Predicate {
            name: "wide_truncated",
            parameter: Type::I64,
            build: |body| body.parameter::<I64>(0).unwrap().truncate::<I32>().ne(0),
            forms: VALUE_FORMS,
            zero_tests: (0, 0),
            masks: 0,
            wraps: 1,
            inputs: &[
                (Value::I64(0), 29),
                (Value::I64(4294967296), 29),
                (Value::I64(4294967297), 17),
            ],
        },
    ]
}

fn predicate_module() -> TestModule {
    let mut fixture = Fixture::new();
    let program = &mut fixture.program;
    for predicate in predicates() {
        for &form in predicate.forms {
            let function = program
                .function(
                    Signature {
                        parameters: vec![predicate.parameter],
                        result: Some(Type::I32),
                    },
                    |body| {
                        let condition = (predicate.build)(&body);
                        form.finish(body, condition)
                    },
                )
                .unwrap();
            program
                .export(&format!("{}_{}", predicate.name, form.name()), function)
                .unwrap();
        }
    }
    fixture.compile()
}

#[derive(Default)]
struct Code {
    i32_eqz: usize,
    i64_eqz: usize,
    masks: usize,
    wraps: usize,
    local_writes: usize,
    calls: usize,
    loads: usize,
}

fn inspect(bytes: &[u8]) -> BTreeMap<String, Code> {
    Validator::new().validate_all(bytes).unwrap();
    let mut exports = BTreeMap::new();
    let mut functions = BTreeMap::new();
    let mut function_index = 0;
    for payload in Parser::new(0).parse_all(bytes) {
        match payload.unwrap() {
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.kind == ExternalKind::Func {
                        exports.insert(export.index, export.name.to_owned());
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                if let Some(name) = exports.remove(&function_index) {
                    let mut code = Code::default();
                    for operator in body.get_operators_reader().unwrap() {
                        match operator.unwrap() {
                            Operator::I32Eqz => code.i32_eqz += 1,
                            Operator::I64Eqz => code.i64_eqz += 1,
                            Operator::I32And => code.masks += 1,
                            Operator::I32WrapI64 => code.wraps += 1,
                            Operator::LocalSet { .. } | Operator::LocalTee { .. } => {
                                code.local_writes += 1;
                            }
                            Operator::Call { .. } => code.calls += 1,
                            Operator::I32Load { .. } => code.loads += 1,
                            _ => {}
                        }
                    }
                    functions.insert(name, code);
                }
                function_index += 1;
            }
            _ => {}
        }
    }
    functions
}

#[test]
fn truth_consumers_omit_boolean_conversion_but_keep_width_and_polarity() {
    let functions = inspect(predicate_module().bytes());
    for predicate in predicates() {
        for &form in predicate.forms {
            let name = format!("{}_{}", predicate.name, form.name());
            let code = &functions[&name];
            assert_eq!((code.i32_eqz, code.i64_eqz), predicate.zero_tests, "{name}");
            assert_eq!(code.masks, predicate.masks, "{name}");
            assert_eq!(code.wraps, predicate.wraps, "{name}");
        }
    }
}

fn shared_numeric_condition() -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &[0xa5; 8]);
    let echo = fixture
        .program
        .function(signature(&[Type::I1], Some(Type::I32)), |body| {
            let value = body.parameter::<I1>(0)?;
            body.return_(value.unsigned().extend::<I32>())
        })
        .unwrap();
    fixture.function(&[Type::I32], Some(Type::I64), |mut body| {
        let condition = body.parameter::<I32>(0)?.and(8).ne(0);
        body.store(state, 0, condition.unsigned().extend::<I8>())?;
        body.if_(&condition, |mut arm| arm.store::<I32>(state, 4, 17))?;
        let received = body.call::<I32>(echo, &[condition.argument()])?;
        body.return_(
            condition
                .select::<I64>(100u64, 200u64)
                .add(condition.unsigned().extend::<I64>())
                .add(received.unsigned().extend::<I64>()),
        )
    })
}

#[test]
fn shared_conditions_keep_one_canonical_value_for_numeric_observers() {
    let functions = inspect(shared_numeric_condition().bytes());
    let code = &functions["run"];
    assert_eq!((code.i32_eqz, code.i64_eqz), (2, 0));
    assert_eq!(code.masks, 1);
    assert_eq!(code.local_writes, 1);
    assert_eq!(code.calls, 1);
}

fn condition_snapshot(initial: u32) -> TestModule {
    let mut fixture = Fixture::new();
    let state = fixture.memory("state", &initial.to_le_bytes());
    fixture.function(&[], Some(Type::I32), |mut body| {
        let condition = body.load::<I32>(state, 0)?.and(8).ne(0);
        body.store::<I32>(state, 0, 0)?;
        body.return_(condition.select::<I32>(17, 29))
    })
}

#[test]
fn an_unshared_condition_preserves_its_read_snapshot_without_a_boolean_local() {
    let functions = inspect(condition_snapshot(8).bytes());
    let code = &functions["run"];
    assert_eq!((code.i32_eqz, code.i64_eqz), (0, 0));
    assert_eq!(code.loads, 1);
    assert_eq!(code.local_writes, 1);
}

fn numeric_switch(value_form: bool) -> TestModule {
    Fixture::new().function(&[Type::I32], Some(Type::I32), |mut body| {
        let selector = body
            .parameter::<I32>(0)?
            .and(8)
            .ne(0)
            .unsigned()
            .extend::<I32>();
        let result = |key| match key {
            Some(1) => 17,
            Some(8) => 88,
            _ => 29,
        };
        if value_form {
            let value =
                body.switch_value::<I32, _>(selector, &[1, 8], |arm, key| arm.yield_(result(key)))?;
            body.return_(value)
        } else {
            body.switch(selector, &[1, 8], |arm, key| arm.return_(result(key)))?;
            body.trap()
        }
    })
}

#[test]
fn switch_selectors_keep_exact_boolean_keys() {
    for value_form in [false, true] {
        let functions = inspect(numeric_switch(value_form).bytes());
        let code = &functions["run"];
        assert_eq!((code.i32_eqz, code.i64_eqz), (2, 0));
    }
}

#[test]
fn numeric_predicates_control_all_conditional_forms_at_runtime() {
    let module = predicate_module();
    let mut instance = module.instantiate();
    for predicate in predicates() {
        for &form in predicate.forms {
            let name = format!("{}_{}", predicate.name, form.name());
            for &(input, expected) in predicate.inputs {
                assert_eq!(
                    instance.call_values(&name, &[input]).unwrap(),
                    Some(Value::I32(expected)),
                    "{name} with {input:?}",
                );
            }
        }
    }
}

#[test]
fn numeric_conditions_share_canonical_values_with_stores() {
    let module = shared_numeric_condition();
    for (input, expected, memory) in [
        (8, 102_i64, [1, 0xa5, 0xa5, 0xa5, 0x11, 0, 0, 0]),
        (0, 200, [0, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]),
    ] {
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i64>((input,)).unwrap(), expected);
        assert_eq!(&instance.memory("state")[..8], memory);
    }
}

#[test]
fn numeric_conditions_preserve_their_memory_snapshot() {
    for (initial, expected) in [(8, 17), (0, 29)] {
        let module = condition_snapshot(initial);
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>(()).unwrap(), expected);
        assert_eq!(&instance.memory("state")[..4], [0, 0, 0, 0]);
    }
}

#[test]
fn numeric_predicates_control_both_switch_forms() {
    for value_form in [false, true] {
        let module = numeric_switch(value_form);
        let mut instance = module.instantiate();
        assert_eq!(instance.call::<i32>((8,)).unwrap(), 17);
        assert_eq!(instance.call::<i32>((0,)).unwrap(), 29);
    }
}

#[test]
#[ignore = "requires Node.js; run the explicit V8 lane"]
fn v8_conditions_preserve_numeric_values_and_memory_snapshots() {
    use crate::wasm::{Input, MemoryBytes, Observation};

    let module = shared_numeric_condition();
    for (input, result, memory) in [
        (8, 102_i64, [1, 0xa5, 0xa5, 0xa5, 0x11, 0, 0, 0]),
        (0, 200, [0, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5, 0xa5]),
    ] {
        let input = Input::call("run", &[Value::I32(input)])
            .with_memories(&[MemoryBytes::new("state", &[0xa5; 8])]);
        assert_eq!(
            module.run_v8(&input),
            Observation::returned(Value::I64(result))
                .with_memories(&[MemoryBytes::new("state", &memory)]),
        );
    }

    let module = condition_snapshot(8);
    let input = Input::call("run", &[]).with_memories(&[MemoryBytes::new("state", &[8, 0, 0, 0])]);
    assert_eq!(
        module.run_v8(&input),
        Observation::returned(Value::I32(17))
            .with_memories(&[MemoryBytes::new("state", &[0, 0, 0, 0])]),
    );
}
