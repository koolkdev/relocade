//! Explicit evaluation when a value's possible trap is itself observable.

use crate::{BuildError, FunctionBuilder, IntType, Operation, Val};

impl FunctionBuilder<'_> {
    /// Requires a value to be evaluated by this point, even when it has no later
    /// use. Evaluation is ordered with stores and other required effects, so a
    /// possible trap occurs before subsequent effects. Existing read snapshots
    /// still apply: reusing the value does not read memory again, and an earlier
    /// overlapping store may already have required its evaluation.
    ///
    /// This does not restore expressions removed by constant folding. Evaluate
    /// the read itself when its access is required independently of a calculation.
    ///
    /// ```
    /// use wasm86_compiler::{MemoryImport, Program, Signature, I32};
    /// let mut program = Program::new();
    /// let memory = program.import_memory(MemoryImport {
    ///     module: "guest".into(), name: "memory".into(), minimum: 1, maximum: None,
    /// });
    /// let function = program.function(Signature {
    ///     parameters: vec![], result: None,
    /// }, |mut body| {
    ///     let value = body.load::<I32>(memory, 65536)?;
    ///     body.evaluate(value)?; // The out-of-bounds read must trap.
    ///     body.store::<I32>(memory, 0, 7)?; // Unreachable after that trap.
    ///     body.return_void()
    /// })?;
    /// program.export("run", function)?;
    /// let bytes = program.compile()?;
    /// # Ok::<(), wasm86_compiler::BuildError>(())
    /// ```
    pub fn evaluate<T: IntType>(&mut self, value: impl Into<Val<T>>) -> Result<(), BuildError> {
        let value = self.operand(value)?;
        self.region.operations.push(Operation::Evaluate(value));
        Ok(())
    }
}
