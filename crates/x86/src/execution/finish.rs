//! Successor edges survive instruction lowering until execution completes.

use wasm86_compiler::{BlockBuilder, BranchCondition, BranchHint, BuildError, Val, I1, I32};

use super::ExecutionBuilder;

/// A successful instruction's destinations. Conditional transfers retain both
/// edges and their prediction until the execution exit is emitted.
pub(crate) enum Successor {
    Unconditional(Val<I32>),
    Conditional {
        condition: Val<I1>,
        hint: Option<BranchHint>,
        taken: Val<I32>,
        fallthrough: Val<I32>,
    },
}

impl From<Val<I32>> for Successor {
    fn from(target: Val<I32>) -> Self {
        Self::Unconditional(target)
    }
}

impl ExecutionBuilder<'_, '_> {
    /// Publishes the completed block, checks its instruction budget and
    /// dispatches the successor on each outgoing edge.
    pub(crate) fn dispatch(self, successor: Successor) -> Result<(), BuildError> {
        let runtime = self.runtime;
        self.finish(successor, true, |body, _| runtime.dispatch(body))
    }

    /// Publishes an instruction and continues decoding within the current block.
    pub(crate) fn complete(
        self,
        successor: Successor,
        continue_execution: impl Fn(BlockBuilder<'_>, &Val<I32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        self.finish(successor, false, continue_execution)
    }

    fn finish(
        self,
        successor: Successor,
        check_budget: bool,
        continue_execution: impl Fn(BlockBuilder<'_>, &Val<I32>) -> Result<(), BuildError>,
    ) -> Result<(), BuildError> {
        let Self {
            mut body,
            state,
            completed,
            ..
        } = self;
        let complete = |mut body: BlockBuilder<'_>, target: &Val<I32>| {
            let count = state
                .publish(&mut body, target, completed)?
                .expect("successful completion retires an instruction");
            if check_budget {
                state.check_instruction_budget(&mut body, &count)?;
            }
            continue_execution(body, target)
        };
        match successor {
            Successor::Unconditional(target) => complete(body, &target),
            Successor::Conditional {
                condition,
                hint,
                taken,
                fallthrough,
            } => {
                let mut condition = BranchCondition::from(condition);
                if let Some(hint) = hint {
                    condition = condition.with_hint(hint);
                }
                body.if_else(
                    condition,
                    |arm| complete(arm, &taken),
                    |arm| complete(arm, &fallthrough),
                )?;
                body.trap()
            }
        }
    }
}
