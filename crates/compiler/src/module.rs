//! WebAssembly sections assembled from the program declarations.
use std::collections::HashMap;

use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, FunctionSection, ImportSection, MemoryType,
    Module, TypeSection,
};

use crate::{emit, FunctionKind, Operation, Program, Terminal, ValueKind};

pub(super) fn encode(program: &Program) -> Vec<u8> {
    let defined: Vec<_> = program
        .functions
        .iter()
        .enumerate()
        .filter_map(|(id, declaration)| match &declaration.kind {
            FunctionKind::Defined(body) => Some((
                id,
                body.as_ref().expect("compilation requires finished bodies"),
            )),
            FunctionKind::Imported { .. } => None,
        })
        .collect();
    let mut used_functions = vec![false; program.functions.len()];
    for (_, function) in &program.exports {
        used_functions[function.0] = true;
    }
    let mut used_memories = vec![false; program.memories.len()];
    for (_, body) in &defined {
        for region in body.region.walk() {
            if let Some(Terminal::TailCall { target, .. }) = &region.terminal {
                used_functions[target.0] = true;
            }
            // Imports follow authored operations, including unused loads.
            for operation in &region.operations {
                let location = match operation {
                    Operation::If { .. } => continue,
                    Operation::Store { location, .. } => *location,
                    Operation::Load(value) => match body.values[*value].kind {
                        ValueKind::Load { location, .. } => location,
                        _ => unreachable!("a load operation names its load value"),
                    },
                };
                used_memories[location.memory.0] = true;
            }
        }
    }
    let imported: Vec<_> = program
        .functions
        .iter()
        .enumerate()
        .filter_map(|(id, declaration)| {
            (used_functions[id] && matches!(declaration.kind, FunctionKind::Imported { .. }))
                .then_some(id)
        })
        .collect();
    let mut function_indices = vec![None; program.functions.len()];
    for (index, id) in imported
        .iter()
        .copied()
        .chain(defined.iter().map(|(id, _)| *id))
        .enumerate()
    {
        function_indices[id] =
            Some(u32::try_from(index).expect("function index fits the Wasm index space"));
    }

    let mut types = TypeSection::new();
    let mut interned = HashMap::new();
    let mut function_types = vec![0; program.functions.len()];
    // Logical signatures can share a Wasm signature. Assign types in definition
    // order, then live import order, independently of hash-map iteration order.
    for id in defined
        .iter()
        .map(|(id, _)| *id)
        .chain(imported.iter().copied())
    {
        let declaration = &program.functions[id];
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
        function_types[id] =
            *interned
                .entry(signature)
                .or_insert_with_key(|(parameters, result)| {
                    let index = types.len();
                    types.ty().function(parameters.iter().copied(), [*result]);
                    index
                });
    }
    let mut module = Module::new();
    module.section(&types);
    let mut imports = ImportSection::new();
    for id in imported {
        let FunctionKind::Imported { module, name } = &program.functions[id].kind else {
            unreachable!("function imports name imported declarations")
        };
        imports.import(module, name, EntityType::Function(function_types[id]));
    }
    let mut memories = vec![None; program.memories.len()];
    let mut memory_index = 0;
    for (index, memory) in program.memories.iter().enumerate() {
        if used_memories[index] {
            // Function and memory indices are separate, even though both kinds
            // of declaration appear in the same import section.
            memories[index] = Some(memory_index);
            memory_index += 1;
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
    let mut functions = FunctionSection::new();
    for (id, _) in &defined {
        functions.function(function_types[*id]);
    }
    module.section(&functions);

    let mut exports = ExportSection::new();
    for (name, function) in &program.exports {
        exports.export(
            name,
            ExportKind::Func,
            function_indices[function.0].expect("an exported function is retained"),
        );
    }
    module.section(&exports);
    let mut code = CodeSection::new();
    for (id, body) in defined {
        let parameters = u32::try_from(program.functions[id].signature.parameters.len())
            .expect("function parameter count fits the Wasm index space");
        code.function(&emit::encode(
            body,
            parameters,
            &memories,
            &function_indices,
        ));
    }
    module.section(&code);
    module.finish()
}
