//! Retention follows calls that survive each function's optimization.

use super::emission::Emission;
use crate::{body::FunctionGraph, ssa::Optimizer, BuiltProgram, FunctionKind};
use std::sync::Arc;

pub(super) struct Definitions {
    program: BuiltProgram,
    optimizer: Arc<Optimizer>,
    pending: Vec<usize>,
    used: Vec<bool>,
    defined: Vec<OptimizedFunction>,
}

pub(super) struct FunctionOptimization {
    id: usize,
    body: FunctionGraph,
    optimizer: Arc<Optimizer>,
}

pub(super) struct OptimizedFunction {
    pub(super) id: usize,
    pub(super) body: crate::body::SsaGraph,
}

impl Definitions {
    pub(super) fn new(program: BuiltProgram) -> Self {
        let optimizer = Arc::new(Optimizer::new(&program));
        let pending = program
            .declarations
            .exports
            .iter()
            .map(|(_, function)| function.0)
            .collect();
        let used = vec![false; program.declarations.functions.len()];
        Self {
            program,
            optimizer,
            pending,
            used,
            defined: Vec::new(),
        }
    }

    pub(super) fn next(&mut self) -> Option<FunctionOptimization> {
        while let Some(id) = self.pending.pop() {
            // Mark before submission, including imports, so shared callees and
            // recursive cycles are visited only once regardless of completion order.
            if std::mem::replace(&mut self.used[id], true)
                || matches!(
                    self.program.declarations.functions[id].kind,
                    FunctionKind::Imported { .. }
                )
            {
                continue;
            }
            let body = self.program.bodies[id]
                .take()
                .expect("reachable definitions completed construction");
            return Some(FunctionOptimization {
                id,
                body,
                optimizer: Arc::clone(&self.optimizer),
            });
        }
        None
    }

    pub(super) fn include(&mut self, function: OptimizedFunction) {
        function
            .body
            .visit_call_targets(|callee| self.pending.push(callee.0), |_| {});
        self.defined.push(function);
    }

    pub(super) fn finish(self) -> Emission {
        Emission::new(self.program.declarations, self.used, self.defined)
    }
}

impl FunctionOptimization {
    pub(super) fn optimize(self) -> OptimizedFunction {
        OptimizedFunction {
            id: self.id,
            body: self.optimizer.optimize(self.body),
        }
    }
}
