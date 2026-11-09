//! Deterministic Wasm declarations and the indices used by function emission.

use std::collections::HashMap;
use wasm_encoder::{
    EntityType, ExportKind, ExportSection, FunctionSection, ImportSection, MemoryType, Module,
    RefType, TableType, TypeSection, ValType,
};

use super::functions::PlacedFunction;
use crate::{emit, FunctionKind, Program, Signature};

/// Owns section declarations and their matching function, memory and table indices.
pub(super) struct Declarations<'a> {
    program: &'a Program,
    types: Types,
    imports: ImportSection,
    functions: FunctionSection,
    encoder: emit::ModuleEncoder,
}

impl<'a> Declarations<'a> {
    /// Definitions arrive in declaration order, independently of job completion.
    pub(super) fn new(
        program: &'a Program,
        used_functions: &[bool],
        defined: &[PlacedFunction],
    ) -> Self {
        let mut declarations = Self {
            program,
            types: Types::default(),
            imports: ImportSection::new(),
            functions: FunctionSection::new(),
            encoder: emit::ModuleEncoder {
                functions: vec![None; program.functions.len()],
                memories: vec![None; program.memories.len()],
                tables: vec![None; program.tables.len()],
                features: program.features,
            },
        };
        declarations.declare_functions(used_functions, defined);
        declarations.import_resources(defined);
        declarations
    }

    fn declare_functions(&mut self, used: &[bool], defined: &[PlacedFunction]) {
        let imported: Vec<_> = self
            .program
            .functions
            .iter()
            .enumerate()
            .filter_map(|(id, declaration)| {
                (used[id] && matches!(declaration.kind, FunctionKind::Imported { .. }))
                    .then_some(id)
            })
            .collect();
        // Wasm places imported functions before definitions in its function index space.
        for (index, id) in imported
            .iter()
            .copied()
            .chain(defined.iter().map(|function| function.id))
            .enumerate()
        {
            self.encoder.functions[id] =
                Some(u32::try_from(index).expect("function index fits the Wasm index space"));
        }

        let mut function_types = vec![0; self.program.functions.len()];
        // Preserve signature interning order: definitions, live imports, then tables.
        for id in defined
            .iter()
            .map(|function| function.id)
            .chain(imported.iter().copied())
        {
            function_types[id] = self.types.function(&self.program.functions[id].signature);
        }
        for id in imported {
            let FunctionKind::Imported { module, name } = &self.program.functions[id].kind else {
                unreachable!("function imports name imported declarations")
            };
            self.imports
                .import(module, name, EntityType::Function(function_types[id]));
        }
        for function in defined {
            self.functions.function(function_types[function.id]);
        }
    }

    fn import_resources(&mut self, defined: &[PlacedFunction]) {
        let mut memories = vec![false; self.program.memories.len()];
        let mut tables = vec![false; self.program.tables.len()];
        for function in defined {
            for memory in &function.body.memories {
                memories[memory.0] = true;
            }
            function
                .body
                .visit_call_targets(|_| {}, |table| tables[table.0] = true);
        }
        self.import_memories(&memories);
        self.import_tables(&tables);
    }

    fn import_memories(&mut self, used: &[bool]) {
        let mut memory_index = 0;
        for (id, memory) in self.program.memories.iter().enumerate() {
            if !used[id] {
                continue;
            }
            // Memory indices are independent of earlier function imports.
            self.encoder.memories[id] = Some(memory_index);
            memory_index += 1;
            self.imports.import(
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

    fn import_tables(&mut self, used: &[bool]) {
        let mut table_index = 0;
        for (id, table) in self.program.tables.iter().enumerate() {
            if !used[id] {
                continue;
            }
            let type_index = self.types.function(&table.signature);
            self.encoder.tables[id] = Some((table_index, type_index));
            table_index += 1;
            self.imports.import(
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
    }

    /// Emits the module prefix and freezes indices for independent body encoders.
    pub(super) fn finish(self) -> (Module, emit::ModuleEncoder) {
        let mut exports = ExportSection::new();
        for (name, function) in &self.program.exports {
            exports.export(
                name,
                ExportKind::Func,
                self.encoder.functions[function.0].expect("an exported function is retained"),
            );
        }
        let mut module = Module::new();
        module.section(&self.types.section);
        if !self.imports.is_empty() {
            module.section(&self.imports);
        }
        module.section(&self.functions);
        module.section(&exports);
        (module, self.encoder)
    }
}

/// Logical function signatures share one deterministic Wasm carrier table.
#[derive(Default)]
struct Types {
    section: TypeSection,
    interned: HashMap<(Vec<ValType>, Vec<ValType>), u32>,
}

impl Types {
    fn function(&mut self, signature: &Signature) -> u32 {
        let parameters = signature
            .parameters
            .iter()
            .copied()
            .map(emit::wasm_type)
            .collect();
        let results = signature
            .results
            .iter()
            .copied()
            .map(emit::wasm_type)
            .collect();
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
