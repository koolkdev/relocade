//! WebAssembly sections assembled from the program declarations.
use std::{collections::HashMap, sync::Arc};

use wasm_encoder::{
    CodeSection, EntityType, ExportKind, ExportSection, FunctionSection, ImportSection, MemoryType,
    Module, RefType, TableType, TypeSection, ValType,
};

use super::functions::PlacedFunction;
use crate::{body::FunctionGraph, emit, FunctionKind, Program};

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

pub(super) struct Emission {
    module: Module,
    functions: Vec<FunctionEmission>,
}

pub(super) struct FunctionEmission {
    id: usize,
    body: FunctionGraph,
    parameters: u32,
    encoder: Arc<emit::ModuleEncoder>,
}

pub(super) struct EncodedFunction {
    id: usize,
    body: wasm_encoder::Function,
}

impl Emission {
    pub(super) fn new(
        program: Program,
        used_functions: Vec<bool>,
        mut defined: Vec<PlacedFunction>,
    ) -> Self {
        // Completion order must not change indices or signature interning.
        defined.sort_unstable_by_key(|function| function.id);
        let mut used_tables = vec![false; program.tables.len()];
        let mut used_memories = vec![false; program.memories.len()];
        for function in &defined {
            for memory in &function.body.memories {
                used_memories[memory.0] = true;
            }
            function
                .body
                .visit_call_targets(|_| {}, |table| used_tables[table.0] = true);
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
            .chain(defined.iter().map(|function| function.id))
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
            .map(|function| function.id)
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
        let mut tables = vec![None; program.tables.len()];
        let mut table_index = 0;
        for (index, table) in program.tables.iter().enumerate() {
            if !used_tables[index] {
                continue;
            }
            let type_index = types.function(
                table
                    .signature
                    .parameters
                    .iter()
                    .copied()
                    .map(emit::wasm_type)
                    .collect(),
                table
                    .signature
                    .results
                    .iter()
                    .copied()
                    .map(emit::wasm_type)
                    .collect(),
            );
            tables[index] = Some((table_index, type_index));
            table_index += 1;
            imports.import(
                &table.module,
                &table.name,
                TableType {
                    element_type: RefType::FUNCREF,
                    minimum: table.minimum.into(),
                    maximum: table.maximum.map(u64::from),
                    table64: false,
                    shared: false,
                },
            );
        }
        let mut functions = FunctionSection::new();
        for function in &defined {
            functions.function(function_types[function.id]);
        }

        let mut exports = ExportSection::new();
        for (name, function) in &program.exports {
            exports.export(
                name,
                ExportKind::Func,
                function_indices[function.0].expect("an exported function is retained"),
            );
        }
        let mut module = Module::new();
        module.section(&types.section);
        if !imports.is_empty() {
            module.section(&imports);
        }
        module.section(&functions);
        module.section(&exports);
        let encoder = Arc::new(emit::ModuleEncoder {
            memories,
            functions: function_indices,
            tables,
            features: program.features,
        });
        let functions = defined
            .into_iter()
            .map(|function| FunctionEmission {
                id: function.id,
                parameters: u32::try_from(
                    program.functions[function.id].signature.parameters.len(),
                )
                .expect("function parameter count fits the Wasm index space"),
                body: function.body,
                encoder: Arc::clone(&encoder),
            })
            .collect();
        Self { module, functions }
    }

    pub(super) fn take_functions(&mut self) -> Vec<FunctionEmission> {
        std::mem::take(&mut self.functions)
    }

    pub(super) fn finish(mut self, mut functions: Vec<EncodedFunction>) -> Vec<u8> {
        functions.sort_unstable_by_key(|function| function.id);
        let mut code = CodeSection::new();
        for function in functions {
            code.function(&function.body);
        }
        self.module.section(&code);
        self.module.finish()
    }
}

impl FunctionEmission {
    pub(super) fn emit(self) -> EncodedFunction {
        EncodedFunction {
            id: self.id,
            body: self.encoder.function(self.body, self.parameters),
        }
    }
}
