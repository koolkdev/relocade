//! WebAssembly sections assembled from the program declarations.
use std::collections::HashMap;

use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, FunctionSection, ImportSection, MemoryType,
    Module, TypeSection, ValType,
};

use crate::{
    body::{BlockItem, Exit, OperationKind},
    emit, place, FunctionKind, Program,
};

/// Logical function signatures share one deterministic Wasm carrier table.
#[derive(Default)]
pub(super) struct Types {
    section: TypeSection,
    interned: HashMap<(Vec<ValType>, Vec<ValType>), u32>,
}

impl Types {
    fn function(&mut self, parameters: Vec<ValType>, results: Vec<ValType>) -> u32 {
        *self
            .interned
            .entry((parameters, results))
            .or_insert_with_key(|(parameters, results)| {
                let index = self.section.len();
                self.section
                    .ty()
                    .function(parameters.iter().copied(), results.iter().copied());
                index
            })
    }
}

pub(super) fn encode(mut program: Program) -> Vec<u8> {
    let mut defined = place::module(&mut program);
    let mut used_functions = vec![false; program.functions.len()];
    let mut bodies = vec![None; program.functions.len()];
    for (id, body) in &defined {
        bodies[*id] = Some(body);
    }
    let mut pending: Vec<_> = program
        .exports
        .iter()
        .map(|(_, function)| function.0)
        .collect();
    let mut used_memories = vec![false; program.memories.len()];
    // Exports are the roots. Calls in retained, reachable blocks keep their
    // transitive helpers, including recursive cycles, and memory imports alive.
    while let Some(id) = pending.pop() {
        if std::mem::replace(&mut used_functions[id], true) {
            continue;
        }
        let Some(body) = bodies[id] else {
            continue;
        };
        for memory in &body.memories {
            used_memories[memory.0] = true;
        }
        let reachable = body.reachable();
        for (index, block) in body.blocks.iter().enumerate() {
            if !reachable[index] {
                continue;
            }
            if let Exit::TailCall { target, .. } = &block.exit {
                pending.push(target.0);
            }
            for item in &block.items {
                if let BlockItem::Effect(effect) = item {
                    if let OperationKind::Call { target } = body.effects[effect.0].operation.kind()
                    {
                        pending.push(target.0);
                    }
                }
            }
        }
    }
    defined.retain(|(id, _)| used_functions[*id]);

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

    let mut types = Types::default();
    let mut function_types = vec![0; program.functions.len()];
    // Logical signatures can share a Wasm signature. Assign types in definition
    // order, then live import order, independently of hash-map iteration order.
    for id in defined
        .iter()
        .map(|(id, _)| *id)
        .chain(imported.iter().copied())
    {
        let declaration = &program.functions[id];
        function_types[id] = types.function(
            declaration
                .signature
                .parameters
                .iter()
                .copied()
                .map(emit::wasm_type)
                .collect::<Vec<_>>(),
            declaration
                .signature
                .results
                .iter()
                .copied()
                .map(emit::wasm_type)
                .collect(),
        );
    }
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
                    shared: memory.shared,
                    page_size_log2: None,
                },
            );
        }
    }
    let mut functions = FunctionSection::new();
    for (id, _) in &defined {
        functions.function(function_types[*id]);
    }

    let mut exports = ExportSection::new();
    for (name, function) in &program.exports {
        exports.export(
            name,
            ExportKind::Func,
            function_indices[function.0].expect("an exported function is retained"),
        );
    }
    let mut code = CodeSection::new();
    for (id, body) in defined {
        let parameters = u32::try_from(program.functions[id].signature.parameters.len())
            .expect("function parameter count fits the Wasm index space");
        code.function(&emit::encode(
            body,
            parameters,
            &memories,
            &function_indices,
            program.features,
        ));
    }
    // Function signatures are interned in deterministic declaration order.
    let mut module = Module::new();
    module.section(&types.section);
    if !imports.is_empty() {
        module.section(&imports);
    }
    module.section(&functions);
    module.section(&exports);
    module.section(&code);
    module.finish()
}
