//! Facts established outside generated code or by a preceding guard.
use crate::{BlockBuilder, BuildError, IntType, Val};

impl BlockBuilder<'_> {
    /// Returns the same runtime value with known logical bits in this scope.
    /// The caller must establish `value & mask == expected & mask` through entry
    /// admission or a preceding guard. This emits no check or bit manipulation.
    /// Bits outside the logical width are ignored; other bits remain unchanged.
    ///
    /// Use the returned value to benefit from the assumption. Existing handles
    /// keep their knowledge. New facts stay in this scope unless passed through
    /// an explicit branch result. Repeating known facts reuses the value.
    /// Constant queries see these facts during construction. Impossible assumptions
    /// add no knowledge, matching refinement on unreachable control-flow paths.
    pub fn assume_bits<T: IntType>(
        &self,
        value: impl Into<Val<T>>,
        mask: u64,
        expected: u64,
    ) -> Result<Val<T>, BuildError> {
        let original = value.into();
        let source = original.checked_expression(&self.arena, self.pending.id)?;
        let value = self
            .arena
            .assume_bits(source, self.pending.current, mask, expected)?;
        if value == source {
            return self.value(original);
        }
        Ok(Val::new(self.arena.clone(), Ok(value)))
    }
}

#[cfg(test)]
mod tests;
