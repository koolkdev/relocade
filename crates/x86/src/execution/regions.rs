//! Nested execution inherits a restart boundary and returns values or dispatches.

use wasm86_compiler::{Arguments, BlockBuilder, BuildError, Results, Val, I1, I32};

use super::ExecutionBuilder;

impl<'module> ExecutionBuilder<'_, 'module> {
    /// Completes a conditional transfer with its own effects and destination.
    /// The taken arm retires this instruction and dispatches; the other arm
    /// continues unchanged. Faults inside the arm retain the entry boundary.
    pub(crate) fn dispatch_if(
        &mut self,
        condition: impl Into<Val<I1>>,
        transfer: impl FnOnce(&mut ExecutionBuilder<'_, 'module>) -> Result<Val<I32>, BuildError>,
    ) -> Result<(), BuildError> {
        let nested = self.nested_builder();
        self.body.if_(condition, |body| {
            let mut arm = nested(body);
            arm.eip = transfer(&mut arm)?;
            arm.completed += 1;
            let runtime = arm.runtime;
            arm.complete(|body, eip| runtime.dispatch(body, eip))
        })
    }

    /// Child register definitions do not merge into the parent. Callers return
    /// values and define the successful results there. This does not roll back
    /// guest memory or CPU backing stores authored inside the child.
    fn nested_builder(
        &self,
    ) -> impl for<'body> Fn(BlockBuilder<'body>) -> ExecutionBuilder<'body, 'module> + use<'module>
    {
        let state = self.state.clone();
        let memory = self.memory.clone();
        let segments = self.segments;
        let segment_override = self.segment_override.clone();
        let address_size = self.address_size;
        let locked = self.locked;
        let x87_opcode = self.x87_opcode.clone();
        let runtime = self.runtime;
        let eip = self.eip.clone();
        let completed = self.completed;
        move |body| ExecutionBuilder {
            body,
            state: state.clone(),
            memory: memory.clone(),
            segments,
            segment_override: segment_override.clone(),
            address_size,
            locked,
            x87_opcode: x87_opcode.clone(),
            runtime,
            eip: eip.clone(),
            completed,
            // A child can contain partial effects of the current instruction,
            // so it cannot restart that instruction from its entry state.
            can_specialize: false,
            observed_cpu: None,
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
