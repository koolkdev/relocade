//! One current SSA value and whether it needs publication by its owner.

use wasm86_compiler::{BlockBuilder, BuildError, IntType, Val};

/// Tracks a current definition and its dirty state on one straight-line build
/// path. Cloning forks this bookkeeping; previously read SSA values stay valid.
/// The owner decides how to publish a changed value.
#[derive(Clone)]
pub(crate) struct TrackedValue<T: IntType> {
    value: Val<T>,
    dirty: bool,
}

impl<T: IntType> TrackedValue<T> {
    /// Captures a value already represented by its owner's external state.
    pub(crate) fn new(
        body: &mut BlockBuilder<'_>,
        value: impl Into<Val<T>>,
    ) -> Result<Self, BuildError> {
        Ok(Self {
            value: body.value(value)?,
            dirty: false,
        })
    }

    /// Introduces a value that its owner has not yet published.
    pub(crate) fn defined(
        body: &mut BlockBuilder<'_>,
        value: impl Into<Val<T>>,
    ) -> Result<Self, BuildError> {
        let mut tracked = Self::new(body, value)?;
        tracked.dirty = true;
        Ok(tracked)
    }

    pub(crate) fn read(&self, body: &mut BlockBuilder<'_>) -> Result<Val<T>, BuildError> {
        body.value(&self.value)
    }

    pub(crate) fn value(&self) -> &Val<T> {
        &self.value
    }

    pub(crate) fn dirty_value(&self) -> Option<&Val<T>> {
        self.dirty.then_some(&self.value)
    }

    /// Repeated definitions of the same expression leave the dirty state alone.
    /// The returned flag lets an external owner order newly changed definitions.
    pub(crate) fn define(
        &mut self,
        body: &mut BlockBuilder<'_>,
        value: impl Into<Val<T>>,
    ) -> Result<bool, BuildError> {
        let value = body.value(value)?;
        if self.read(body)?.same_expression(&value) {
            return Ok(false);
        }
        self.value = value;
        self.dirty = true;
        Ok(true)
    }

    /// The owner has synchronized the current value with its external state.
    pub(crate) fn mark_clean(&mut self) {
        self.dirty = false;
    }
}

#[cfg(test)]
mod tests;
