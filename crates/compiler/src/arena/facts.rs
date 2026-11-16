//! Attach established facts and read facts retained during value construction.
use super::FunctionArena;
use crate::{
    body::{Bits, BlockId, ValueDefinition},
    literal::Literal,
    BuildError,
};

impl FunctionArena {
    pub(crate) fn assume_bits(
        &self,
        source: usize,
        block: BlockId,
        mask: u64,
        expected: u64,
    ) -> Result<usize, BuildError> {
        let mut arena = self.0.borrow_mut();
        let values = &mut arena.as_mut().ok_or(BuildError::BodyClosed)?.graph.values;
        let known = Bits::constant(expected, mask & values[source].ty.mask());
        let previous = values.known_bits(source);
        // Construction also visits continuations of unconditional exits.
        // Impossible assumptions add no knowledge on those paths.
        if previous.conflicts(known) || previous.union(known) == previous {
            return Ok(source);
        }
        Ok(values.refined_alias(source, block, known))
    }

    pub(crate) fn constant_literal(&self, value: usize) -> Result<Option<Literal>, BuildError> {
        let arena = self.0.borrow();
        let values = &arena.as_ref().ok_or(BuildError::BodyClosed)?.graph.values;
        if let ValueDefinition::Literal(bits) = values[value].definition {
            return Ok(Some(bits));
        }
        Ok(values
            .known_bits(value)
            .known(values[value].ty.mask())
            .map(Literal::from))
    }
}
