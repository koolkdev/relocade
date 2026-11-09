//! Export-rooted IR construction using frozen module declarations.

use std::sync::Arc;

use crate::{
    body::FunctionGraph, BlockBuilder, BuildError, BuiltProgram, Func, FunctionKind,
    ModuleDeclarations, Program,
};

pub(crate) type BuildBody = Box<dyn FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>>;

struct Construction {
    program: BuiltProgram,
    builders: Vec<Option<BuildBody>>,
    visited: Vec<bool>,
    pending: Vec<Func>,
}

struct FunctionConstruction {
    declarations: Arc<ModuleDeclarations>,
    function: Func,
    build: BuildBody,
}

struct BuiltFunction {
    function: Func,
    body: FunctionGraph,
}

impl Program {
    pub(crate) fn build_reachable_functions(self) -> Result<BuiltProgram, BuildError> {
        let mut construction = Construction::new(self);
        while let Some(function) = construction.next()? {
            construction.include(function.build()?);
        }
        Ok(construction.program)
    }
}

impl Construction {
    fn new(program: Program) -> Self {
        let pending = program
            .declarations
            .exports
            .iter()
            .map(|(_, function)| *function)
            .collect();
        let count = program.declarations.functions.len();
        Self {
            program: BuiltProgram {
                declarations: Arc::new(program.declarations),
                bodies: (0..count).map(|_| None).collect(),
            },
            builders: program.builders,
            visited: vec![false; count],
            pending,
        }
    }

    fn next(&mut self) -> Result<Option<FunctionConstruction>, BuildError> {
        while let Some(function) = self.pending.pop() {
            // Mark before submission so shared callees and recursive cycles build once.
            if std::mem::replace(&mut self.visited[function.0], true)
                || matches!(
                    self.program.declarations.functions[function.0].kind,
                    FunctionKind::Imported { .. }
                )
            {
                continue;
            }
            return Ok(Some(FunctionConstruction {
                declarations: Arc::clone(&self.program.declarations),
                function,
                build: self.builders[function.0]
                    .take()
                    .ok_or(BuildError::MissingBody)?,
            }));
        }
        Ok(None)
    }

    fn include(&mut self, function: BuiltFunction) {
        function
            .body
            .visit_call_targets(|callee| self.pending.push(callee), |_| {});
        self.program.bodies[function.function.0] = Some(function.body);
    }
}

impl FunctionConstruction {
    fn build(self) -> Result<BuiltFunction, BuildError> {
        Ok(BuiltFunction {
            function: self.function,
            body: super::build_function(&self.declarations, self.function, self.build)?,
        })
    }
}
