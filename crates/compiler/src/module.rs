//! WebAssembly sections assembled from the program declarations.
use std::collections::HashMap;

use wasm_encoder::{CodeSection, ExportKind, ExportSection, FunctionSection, Module, TypeSection};

use crate::{emit, Program};

pub(super) fn encode(program: &Program) -> Vec<u8> {
    let mut module = Module::new();
    let mut types = TypeSection::new();
    let mut functions = FunctionSection::new();
    let mut interned = HashMap::new();
    for declaration in &program.functions {
        let signature = (
            declaration
                .signature
                .parameters
                .iter()
                .copied()
                .map(emit::wasm_type)
                .collect::<Vec<_>>(),
            emit::wasm_type(declaration.signature.result),
        );
        // Different logical signatures can use the same Wasm signature. Assign
        // its index on first declaration, independent of hash-map iteration order.
        let index = *interned
            .entry(signature)
            .or_insert_with_key(|(parameters, result)| {
                let index = types.len();
                types.ty().function(parameters.iter().copied(), [*result]);
                index
            });
        functions.function(index);
    }
    module.section(&types);
    module.section(&functions);

    let mut exports = ExportSection::new();
    for (name, function) in &program.exports {
        exports.export(
            name,
            ExportKind::Func,
            u32::try_from(function.0).expect("function index fits the Wasm index space"),
        );
    }
    module.section(&exports);

    let mut code = CodeSection::new();
    for declaration in &program.functions {
        let body = declaration
            .body
            .as_ref()
            .expect("compilation requires finished bodies");
        let parameters = u32::try_from(declaration.signature.parameters.len())
            .expect("function parameter count fits the Wasm index space");
        code.function(&emit::encode(body, parameters));
    }
    module.section(&code);
    module.finish()
}
