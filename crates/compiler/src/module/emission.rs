//! Function emission jobs and assembly of their bodies in declaration order.

use std::sync::Arc;
use wasm_encoder::{CodeSection, Module};

use super::{declarations::Declarations, functions::OptimizedFunction};
use crate::{body::SsaGraph, emit, ModuleDeclarations};

pub(super) struct Emission {
    module: Module,
    functions: Vec<FunctionEmission>,
}

pub(super) struct FunctionEmission {
    id: usize,
    body: SsaGraph,
    parameters: u32,
    encoder: Arc<emit::ModuleEncoder>,
}

pub(super) struct EncodedFunction {
    id: usize,
    body: wasm_encoder::Function,
}

impl Emission {
    pub(super) fn new(
        program: Arc<ModuleDeclarations>,
        used_functions: Vec<bool>,
        mut defined: Vec<OptimizedFunction>,
    ) -> Self {
        // Completion order must not change indices or signature interning.
        defined.sort_unstable_by_key(|function| function.id);
        let (module, encoder) = Declarations::new(&program, &used_functions, &defined).finish();
        let encoder = Arc::new(encoder);
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
