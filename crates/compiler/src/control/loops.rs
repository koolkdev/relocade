//! Typed loop entry and exit edges within the structured control tree.
use std::marker::PhantomData;

use super::{Label, Target};
use crate::{
    results, Arguments, BuildError, FunctionBuilder, Operation, Results, Value, ValueKind,
};

/// The two destinations visible within a loop and its descendants.
/// `again` supplies the next iteration's inputs; `exit` supplies the loop result.
/// Each label keeps the same body and scope rules as a block label.
pub struct LoopLabels<P: Results, R: Results> {
    pub again: Label<P>,
    pub exit: Label<R>,
}

impl FunctionBuilder<'_> {
    /// Builds a loop with separate logical input and result shapes.
    ///
    /// Initial values enter once. A branch to `labels.again` evaluates the entire
    /// next input tuple before starting another iteration. A branch to
    /// `labels.exit`, or a direct `yield_`, completes the loop with its result.
    /// A unit result may fall through. Inputs and labels are confined to this
    /// loop and its descendants; only the result is visible afterwards.
    /// A nonempty result requires at least one exit that supplies it.
    /// Errors discard the loop and leave the parent builder usable.
    ///
    /// ```
    /// use wasm86_compiler::{Program, Signature, Type, I1, I32, I64};
    /// let mut program = Program::new();
    /// let function = program.function(Signature {
    ///     parameters: vec![Type::I32], results: vec![Type::I64, Type::I1],
    /// }, |mut body| {
    ///     let count = body.parameter::<I32>(0)?;
    ///     let result = body.loop_::<(I32, I64), (I64, I1)>((count, 0u64),
    ///         |mut iteration, labels, (remaining, sum)| {
    ///             iteration.if_(remaining.eq(0), |done| {
    ///                 done.branch(&labels.exit, (&sum, true))
    ///             })?;
    ///             iteration.branch(&labels.again, (
    ///                 remaining.sub(1), sum.add(remaining.unsigned().extend::<I64>()),
    ///             ))
    ///         },
    ///     )?;
    ///     body.return_(result)
    /// })?;
    /// program.export("sum", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn loop_<P: Results, R: Results>(
        &mut self,
        initial: impl Into<Arguments>,
        build: impl FnOnce(FunctionBuilder<'_>, LoopLabels<P, R>, P::Values) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let input_types = results::types::<P>();
        let initial = self.result_arguments(initial, &input_types)?;
        let target = self.result_target::<R>();
        let scope = self.arena.child_scope(self.region.id)?;
        let inputs = input_types
            .into_iter()
            .enumerate()
            .map(|(component, ty)| {
                self.arena.intern(Value {
                    ty,
                    kind: ValueKind::LoopInput {
                        region: scope,
                        component,
                    },
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let labels = LoopLabels {
            again: Label {
                arena: self.arena.clone(),
                scope,
                target: Target::entry(target.target.site),
                shape: PhantomData,
            },
            exit: Label {
                arena: self.arena.clone(),
                scope,
                target: target.target,
                shape: PhantomData,
            },
        };
        let region = self.build_region(scope, Some(&target), |iteration| {
            let current = results::bind::<P>(&iteration, &inputs);
            build(iteration, labels, current)
        })?;
        let outputs = self.join_outputs(&target, [&region])?;
        let values = results::bind::<R>(self, &outputs);
        self.region.operations.push(Operation::Loop {
            initial,
            inputs,
            region,
            outputs,
        });
        Ok(values)
    }
}
