//! Nested execution inherits a restart boundary and returns explicit values.

use wasm86_compiler::{Arguments, BuildError, FunctionBuilder, LoopLabels, Results, Val, I1};

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

    pub(super) fn loop_<P: Results, R: Results>(
        &mut self,
        initial: impl Into<Arguments>,
        build: impl FnOnce(
            ExecutionBuilder<'_, 'module>,
            LoopLabels<P, R>,
            P::Values,
        ) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let nested = self.nested_builder();
        self.body.loop_::<P, R>(initial, |body, labels, values| {
            build(nested(body), labels, values)
        })
    }

    pub(super) fn if_value<R: Results>(
        &mut self,
        condition: impl Into<Val<I1>>,
        then_build: impl FnOnce(ExecutionBuilder<'_, 'module>) -> Result<(), BuildError>,
        else_build: impl FnOnce(ExecutionBuilder<'_, 'module>) -> Result<(), BuildError>,
    ) -> Result<R::Values, BuildError> {
        let nested = self.nested_builder();
        self.body.if_value::<R>(
            condition,
            |body| then_build(nested(body)),
            |body| else_build(nested(body)),
        )
    }
}
