//! Export-rooted IR construction, before effect inference and placement.

use crate::{BlockBuilder, BuildError, FunctionKind, Program};

pub(crate) type BuildBody = Box<dyn FnOnce(BlockBuilder<'_>) -> Result<(), BuildError>>;

impl Program {
    pub(crate) fn build_reachable_functions(&mut self) -> Result<(), BuildError> {
        let mut visited = Vec::new();
        let mut pending = Vec::new();
        let mut exports = 0;
        loop {
            // A callback may declare helpers or add exports of its own.
            pending.extend(
                self.exports[exports..]
                    .iter()
                    .map(|(_, function)| *function),
            );
            exports = self.exports.len();
            let Some(function) = pending.pop() else {
                return Ok(());
            };
            visited.resize(self.functions.len(), false);
            if std::mem::replace(&mut visited[function.0], true) {
                continue;
            }
            if matches!(self.functions[function.0].kind, FunctionKind::Pending(_)) {
                let FunctionKind::Pending(build) =
                    std::mem::replace(&mut self.functions[function.0].kind, FunctionKind::Building)
                else {
                    unreachable!("the body builder is taken once")
                };
                self.build_function(function, build)?;
            }
            match &self.functions[function.0].kind {
                FunctionKind::Built(body) => {
                    body.visit_call_targets(|function| pending.push(function), |_| {})
                }
                FunctionKind::Declared => return Err(BuildError::MissingBody),
                FunctionKind::Imported { .. } => {}
                FunctionKind::Pending(_) | FunctionKind::Building => {
                    unreachable!("reachable body was built")
                }
            }
        }
    }
}
