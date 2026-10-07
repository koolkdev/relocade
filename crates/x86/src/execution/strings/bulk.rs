//! Native repeated transfers reuse the complete operand proofs.

use super::{ExecutionBuilder, StringOperand};
use crate::{memory::Intent, register::RegisterType};
use wasm86_compiler::{AtLeast, BuildError, Val, I1, I32};

impl ExecutionBuilder<'_, '_> {
    /// Copies complete repeated elements only when memmove and scalar MOVS agree.
    /// Partial physical overlap retains the relative scalar loop, including aliases.
    pub(crate) fn rep_movs<T: RegisterType>(
        &mut self,
        source: &StringOperand,
        destination: &StringOperand,
        count: &Val<I32>,
    ) -> Result<Val<I1>, BuildError> {
        assert!(matches!(destination.intent, Intent::Write));
        let source = &source
            .relative
            .as_ref()
            .expect("a repeated copy needs resolved operands")
            .physical_start;
        let destination = &destination
            .relative
            .as_ref()
            .expect("a repeated copy needs resolved operands")
            .physical_start;
        let bytes = count.mul(T::BYTES);
        // Modular distances also cover spans ending exactly at 2^32. The range
        // proof already excludes wrapping physical spans.
        let disjoint = destination
            .sub(source)
            .unsigned()
            .ge(&bytes)
            .and(source.sub(destination).unsigned().ge(&bytes));
        let memory = self.memory.as_ref().unwrap().memory();
        self.body.if_value::<I1>(
            source.eq(destination).or(disjoint),
            |mut copy| {
                copy.if_(count.ne(0), |mut nonempty| {
                    memory.copy(&mut nonempty, destination, source, &bytes)
                })?;
                copy.yield_(true)
            },
            |overlap| overlap.yield_(false),
        )
    }

    /// Fills a complete resolved destination with the repeated little-endian element.
    pub(crate) fn rep_stos<T: RegisterType>(
        &mut self,
        destination: &StringOperand,
        value: &Val<T>,
        count: &Val<I32>,
    ) -> Result<Val<I1>, BuildError>
    where
        I32: AtLeast<T>,
    {
        assert!(matches!(destination.intent, Intent::Write));
        let destination = &destination
            .relative
            .as_ref()
            .expect("a repeated fill needs a resolved operand")
            .physical_start;
        let memory = self.memory.as_ref().unwrap().memory();
        self.body.if_(count.ne(0), |mut nonempty| {
            memory.fill(&mut nonempty, destination, value, &count.mul(T::BYTES))
        })?;
        self.body.value(true)
    }
}
