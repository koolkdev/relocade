//! WebAssembly sections assembled from the program declarations.
use std::collections::HashMap;

use wasm_encoder::{
    CodeSection, ExportKind, ExportSection, FunctionSection, ImportSection, MemoryType, Module,
    TypeSection,
};

use crate::{emit, Operation, Program, ValueKind};

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

    let mut used_memories = vec![false; program.memories.len()];
    for declaration in &program.functions {
        let body = declaration
            .body
            .as_ref()
            .expect("compilation requires finished bodies");
        // Imports follow authored operations, including unused loads. Looking at
        // emitted instructions instead would change the module's binding contract.
        for operation in &body.operations {
            let location = match *operation {
                Operation::Store { location, .. } => location,
                Operation::Load(value) => match body.values[value].kind {
                    ValueKind::Load { location, .. } => location,
                    _ => unreachable!("a load operation names its load value"),
                },
            };
            used_memories[location.memory.0] = true;
        }
    }
    let mut memories = vec![None; program.memories.len()];
    let mut imports = ImportSection::new();
    for (index, memory) in program.memories.iter().enumerate() {
        if used_memories[index] {
            memories[index] = Some(imports.len());
            imports.import(
                &memory.module,
                &memory.name,
                MemoryType {
                    minimum: u64::from(memory.minimum),
                    maximum: memory.maximum.map(u64::from),
                    memory64: false,
                    shared: false,
                    page_size_log2: None,
                },
            );
        }
    }
    if !imports.is_empty() {
        module.section(&imports);
    }
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
        code.function(&emit::encode(body, parameters, &memories));
    }
    module.section(&code);
    module.finish()
}
