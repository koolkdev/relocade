use std::collections::BTreeMap;

use super::ModuleFile;
use wasm86_compiler::{
    BuildError, FunctionBuilder, MemoryImport, Program, Signature, Type, Val, I1, I16, I32, I64, I8,
};
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
    inputs: &'static [(&'static str, &'static str)],
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
            inputs: &[("i32:0", "29\n"), ("i32:8", "17\n"), ("i32:16", "29\n")],
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
                ("i32:0", "29\n"),
                ("i32:2", "17\n"),
                ("i32:-2147483648", "17\n"),
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
            inputs: &[("i32:0", "17\n"), ("i32:8", "29\n"), ("i32:16", "17\n")],
        },
        Predicate {
            name: "byte_wrap",
            parameter: Type::I8,
            build: |body| body.parameter::<I8>(0).unwrap().add(1).ne(0),
            forms: VALUE_FORMS,
            zero_tests: (0, 0),
            masks: 1,
            wraps: 0,
            inputs: &[("i32:0", "17\n"), ("i32:254", "17\n"), ("i32:255", "29\n")],
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
                ("i32:0", "17\n"),
                ("i32:65534", "17\n"),
                ("i32:65535", "29\n"),
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
            inputs: &[("i32:0", "29\n"), ("i32:2", "29\n"), ("i32:3", "17\n")],
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
                ("i64:0", "29\n"),
                ("i64:4294967296", "17\n"),
                ("i64:-9223372036854775808", "17\n"),
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
                ("i64:0", "29\n"),
                ("i64:4294967296", "29\n"),
                ("i64:4294967297", "17\n"),
            ],
        },
    ]
}

fn predicate_module() -> Vec<u8> {
    let mut program = Program::new();
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
    program.compile().unwrap()
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
    let functions = inspect(&predicate_module());
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

fn shared_numeric_condition() -> Vec<u8> {
    let mut program = Program::new();
    let state = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    });
    let echo = program
        .function(
            Signature {
                parameters: vec![Type::I1],
                result: Some(Type::I32),
            },
            |body| {
                let value = body.parameter::<I1>(0)?;
                body.return_(value.unsigned().extend::<I32>())
            },
        )
        .unwrap();
    let run = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                result: Some(Type::I64),
            },
            |mut body| {
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
            },
        )
        .unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

#[test]
fn shared_conditions_keep_one_canonical_value_for_numeric_observers() {
    let functions = inspect(&shared_numeric_condition());
    let code = &functions["run"];
    assert_eq!((code.i32_eqz, code.i64_eqz), (2, 0));
    assert_eq!(code.masks, 1);
    assert_eq!(code.local_writes, 1);
    assert_eq!(code.calls, 1);
}

fn condition_snapshot() -> Vec<u8> {
    let mut program = Program::new();
    let state = program.import_memory(MemoryImport {
        module: "test".into(),
        name: "state".into(),
        minimum: 1,
        maximum: None,
    });
    let run = program
        .function(
            Signature {
                parameters: vec![],
                result: Some(Type::I32),
            },
            |mut body| {
                let condition = body.load::<I32>(state, 0)?.and(8).ne(0);
                body.store::<I32>(state, 0, 0)?;
                body.return_(condition.select::<I32>(17, 29))
            },
        )
        .unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

#[test]
fn an_unshared_condition_preserves_its_read_snapshot_without_a_boolean_local() {
    let functions = inspect(&condition_snapshot());
    let code = &functions["run"];
    assert_eq!((code.i32_eqz, code.i64_eqz), (0, 0));
    assert_eq!(code.loads, 1);
    assert_eq!(code.local_writes, 1);
}

fn numeric_switch(value_form: bool) -> Vec<u8> {
    let mut program = Program::new();
    let run = program
        .function(
            Signature {
                parameters: vec![Type::I32],
                result: Some(Type::I32),
            },
            |mut body| {
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
                    let value = body.switch_value::<I32, _>(selector, &[1, 8], |arm, key| {
                        arm.yield_(result(key))
                    })?;
                    body.return_(value)
                } else {
                    body.switch(selector, &[1, 8], |arm, key| arm.return_(result(key)))?;
                    body.trap()
                }
            },
        )
        .unwrap();
    program.export("run", run).unwrap();
    program.compile().unwrap()
}

#[test]
fn switch_selectors_keep_exact_boolean_keys() {
    for value_form in [false, true] {
        let functions = inspect(&numeric_switch(value_form));
        let code = &functions["run"];
        assert_eq!((code.i32_eqz, code.i64_eqz), (2, 0));
    }
}

pub(super) fn check_execution(flags: &[&str]) {
    let module = ModuleFile::new(&predicate_module());
    for predicate in predicates() {
        for &form in predicate.forms {
            let name = format!("{}_{}", predicate.name, form.name());
            for &(input, expected) in predicate.inputs {
                module.check(flags, "execute.mjs", &[&name, input], expected);
            }
        }
    }
    let shared = ModuleFile::new(&shared_numeric_condition());
    shared.check(
        flags,
        "execute-memory.mjs",
        &["state:a5a5a5a5a5a5a5a5", "--", "i32:8"],
        "102\nstate:01a5a5a511000000\n",
    );
    shared.check(
        flags,
        "execute-memory.mjs",
        &["state:a5a5a5a5a5a5a5a5", "--", "i32:0"],
        "200\nstate:00a5a5a5a5a5a5a5\n",
    );
    let snapshot = ModuleFile::new(&condition_snapshot());
    snapshot.check(
        flags,
        "execute-memory.mjs",
        &["state:08000000"],
        "17\nstate:00000000\n",
    );
    snapshot.check(
        flags,
        "execute-memory.mjs",
        &["state:00000000"],
        "29\nstate:00000000\n",
    );
    for value_form in [false, true] {
        let switch = ModuleFile::new(&numeric_switch(value_form));
        switch.check(flags, "execute.mjs", &["run", "i32:8"], "17\n");
        switch.check(flags, "execute.mjs", &["run", "i32:0"], "29\n");
    }
}
