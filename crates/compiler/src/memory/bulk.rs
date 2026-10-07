//! Ordered byte transfers with runtime lengths.

use crate::{body::Operation, BlockBuilder, BuildError, Mem, Val, I32};

impl BlockBuilder<'_> {
    /// Fills a byte range with the low byte of `value` using Wasm `memory.fill`.
    /// The unsigned range does not wrap; an out-of-bounds range traps.
    /// Transfers retain construction order and preserve earlier read snapshots.
    pub fn memory_fill(
        &mut self,
        memory: Mem,
        destination: impl Into<Val<I32>>,
        value: impl Into<Val<I32>>,
        bytes: impl Into<Val<I32>>,
    ) -> Result<(), BuildError> {
        let destination = self.operand(destination)?;
        let value = self.operand(value)?;
        let bytes = self.operand(bytes)?;
        self.require_memory(memory)?;
        self.execute(
            Operation::memory_fill(memory, destination, value, bytes),
            &[],
        )?;
        Ok(())
    }

    /// Copies a byte range using Wasm `memory.copy`, including between memories.
    /// Overlap within one memory has memmove semantics. Unsigned ranges do not
    /// wrap; an out-of-bounds range traps before any bytes change.
    /// Transfers retain construction order and preserve earlier read snapshots.
    pub fn memory_copy(
        &mut self,
        destination_memory: Mem,
        destination: impl Into<Val<I32>>,
        source_memory: Mem,
        source: impl Into<Val<I32>>,
        bytes: impl Into<Val<I32>>,
    ) -> Result<(), BuildError> {
        let destination = self.operand(destination)?;
        let source = self.operand(source)?;
        let bytes = self.operand(bytes)?;
        self.require_memory(destination_memory)?;
        self.require_memory(source_memory)?;
        self.execute(
            Operation::memory_copy(
                destination_memory,
                source_memory,
                destination,
                source,
                bytes,
            ),
            &[],
        )?;
        Ok(())
    }
}
