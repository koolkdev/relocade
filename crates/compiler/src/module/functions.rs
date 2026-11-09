//! Retention follows calls that survive each function's placement.

use super::emission::Emission;
use crate::{body::FunctionGraph, place::Placement, FunctionKind, Program};
use std::sync::Arc;

pub(super) struct Definitions {
    program: Program,
    placement: Arc<Placement>,
    pending: Vec<usize>,
    used: Vec<bool>,
    defined: Vec<PlacedFunction>,
}

pub(super) struct FunctionPlacement {
    id: usize,
    body: FunctionGraph,
    placement: Arc<Placement>,
}

pub(super) struct PlacedFunction {
    pub(super) id: usize,
    pub(super) body: FunctionGraph,
}

impl Definitions {
    pub(super) fn new(program: Program) -> Self {
        let placement = Arc::new(Placement::new(&program));
        let pending = program
            .exports
            .iter()
            .map(|(_, function)| function.0)
            .collect();
        let used = vec![false; program.functions.len()];
        Self {
            program,
            placement,
            pending,
            used,
            defined: Vec::new(),
        }
    }

    pub(super) fn next(&mut self) -> Option<FunctionPlacement> {
        while let Some(id) = self.pending.pop() {
            // Mark before submission, including imports, so shared callees and
            // recursive cycles are visited only once regardless of completion order.
            if std::mem::replace(&mut self.used[id], true)
                || matches!(
                    self.program.functions[id].kind,
                    FunctionKind::Imported { .. }
                )
            {
                continue;
            }
            let FunctionKind::Built(body) =
                std::mem::replace(&mut self.program.functions[id].kind, FunctionKind::Declared)
            else {
                unreachable!("reachable definitions completed construction")
            };
            return Some(FunctionPlacement {
                id,
                body,
                placement: Arc::clone(&self.placement),
            });
        }
        None
    }

    pub(super) fn include(&mut self, function: PlacedFunction) {
        function
            .body
            .visit_call_targets(|callee| self.pending.push(callee.0), |_| {});
        self.defined.push(function);
    }

    pub(super) fn finish(self) -> Emission {
        Emission::new(self.program, self.used, self.defined)
    }
}

impl FunctionPlacement {
    pub(super) fn place(self) -> PlacedFunction {
        PlacedFunction {
            id: self.id,
            body: self.placement.place(self.body),
        }
    }
}
