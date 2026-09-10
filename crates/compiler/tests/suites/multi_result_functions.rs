use crate::fixture::{signature, Fixture};
use crate::wasm::{Call, Callback, Input, MemoryBytes, Observation, TestModule, Value};
use wasm86_compiler::{
    BuildError, Func, FunctionImport, Mem, Program, Type, Val, I1, I16, I32, I64, I8,
};
use wasmparser::{ExternalKind, Operator, Parser, Payload, TypeRef, ValType, Validator};

#[path = "multi_result_functions/effects.rs"]
mod effects;
#[path = "multi_result_functions/placement.rs"]
mod placement;
#[path = "multi_result_functions/results.rs"]
mod results;
#[path = "multi_result_functions/validation.rs"]
mod validation;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Event {
    Call,
    Store(u64),
    If,
    Else,
    End,
}

#[derive(Default)]
struct Code {
    results: Vec<ValType>,
    calls: usize,
    tails: usize,
    drops: usize,
    events: Vec<Event>,
}

fn inspect(module: &TestModule, entry: &str) -> Code {
    Validator::new().validate_all(module.bytes()).unwrap();
    let mut types = Vec::new();
    let mut functions = Vec::new();
    let mut imports = 0;
    let mut exported = None;
    let mut current = 0;
    for payload in Parser::new(0).parse_all(module.bytes()) {
        match payload.unwrap() {
            Payload::TypeSection(section) => {
                for ty in section.into_iter_err_on_gc_types() {
                    types.push(ty.unwrap().results().to_vec());
                }
            }
            Payload::ImportSection(section) => {
                for import in section {
                    if let TypeRef::Func(ty) = import.unwrap().ty {
                        functions.push(ty);
                        imports += 1;
                    }
                }
            }
            Payload::FunctionSection(section) => {
                functions.extend(section.into_iter().map(Result::unwrap));
            }
            Payload::ExportSection(section) => {
                for export in section {
                    let export = export.unwrap();
                    if export.name == entry {
                        assert_eq!(export.kind, ExternalKind::Func);
                        exported = Some(export.index);
                    }
                }
            }
            Payload::CodeSectionEntry(body) => {
                let index = current + imports;
                current += 1;
                if Some(index) != exported {
                    continue;
                }
                let mut code = Code {
                    results: types[functions[index as usize] as usize].clone(),
                    ..Code::default()
                };
                for operator in body.get_operators_reader().unwrap() {
                    match operator.unwrap() {
                        Operator::Call { .. } => {
                            code.calls += 1;
                            code.events.push(Event::Call);
                        }
                        Operator::ReturnCall { .. } => code.tails += 1,
                        Operator::Drop => code.drops += 1,
                        Operator::I32Store { memarg }
                        | Operator::I32Store8 { memarg }
                        | Operator::I32Store16 { memarg }
                        | Operator::I64Store { memarg } => {
                            code.events.push(Event::Store(memarg.offset));
                        }
                        Operator::If { .. } => code.events.push(Event::If),
                        Operator::Else => code.events.push(Event::Else),
                        Operator::End => code.events.push(Event::End),
                        _ => {}
                    }
                }
                return code;
            }
            _ => {}
        }
    }
    panic!("the fixture exports a defined function named {entry}")
}
