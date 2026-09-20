//! Nested execution inherits a restart boundary and returns explicit values.

use wasm86_compiler::{Arguments, BuildError, FunctionBuilder, Results, Val, I1};

use super::ExecutionBuilder;

impl<'module> ExecutionBuilder<'_, 'module> {
    /// Child register definitions do not merge into the parent. Callers return
    /// values and define the successful results there. This does not roll back
    /// guest memory or CPU backing stores authored inside the child.
    fn nested_builder(
        &self,
    ) -> impl for<'body> Fn(FunctionBuilder<'body>) -> ExecutionBuilder<'body, 'module> + use<'module>
    {
        let state = self.state.clone();
        let memory = self.memory;
        let segments = self.segments;
        let segment_override = self.segment_override.clone();
        let address_size = self.address_size;
        let runtime = self.runtime;
        let eip = self.eip.clone();
        let completed = self.completed;
        move |body| ExecutionBuilder {
            body,
            state: state.clone(),
            memory,
            segments,
            segment_override: segment_override.clone(),
            address_size,
            runtime,
            eip: eip.clone(),
            completed,
        }
    }

    /// Checks `done` before the first and every later iteration. An already
    /// completed loop returns its initial values; otherwise `step` supplies the
    /// next iteration's values. The condition only observes carried values.
    /// Child register definitions do not merge; define successful results in
    /// the parent from the returned values. Earlier stores survive faults.
    pub(crate) fn loop_until<P: Results>(
        &mut self,
        initial: impl Into<Arguments>,
        done: impl FnOnce(&P::Values) -> Val<I1>,
        step: impl FnOnce(
            &mut ExecutionBuilder<'_, 'module>,
            P::Values,
        ) -> Result<P::Values, BuildError>,
    ) -> Result<P::Values, BuildError>
    where
        P::Values: Clone + Into<Arguments>,
    {
        let nested = self.nested_builder();
        self.body.loop_::<P, P>(initial, |body, labels, values| {
            let mut iteration = nested(body);
            iteration
                .body
                .branch_if(done(&values), &labels.exit, values.clone())?;
            let next = step(&mut iteration, values)?;
            iteration.body.branch(&labels.again, next)
        })
    }

    /// Runs one arm and returns its explicit values. As with loops, child
    /// register definitions do not merge into the parent and stores persist.
    pub(crate) fn if_value<R: Results>(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(&mut ExecutionBuilder<'_, 'module>) -> Result<R::Values, BuildError>,
        else_build: impl FnOnce(&mut ExecutionBuilder<'_, 'module>) -> Result<R::Values, BuildError>,
    ) -> Result<R::Values, BuildError>
    where
        R::Values: Into<Arguments>,
    {
        let nested = self.nested_builder();
        self.body.if_value::<R>(
            condition,
            |body| {
                let mut arm = nested(body);
                let values = then_build(&mut arm)?;
                arm.body.yield_(values)
            },
            |body| {
                let mut arm = nested(body);
                let values = else_build(&mut arm)?;
                arm.body.yield_(values)
            },
        )
    }
}
