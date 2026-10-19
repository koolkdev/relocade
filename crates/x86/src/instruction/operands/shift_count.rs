//! Packed shift counts retain the access width of their encoded operand.

use super::{Input, MmxLocation, VectorAlignment, XmmLocation};
use crate::execution::ExecutionBuilder;
use relocade_compiler::{BuildError, Val, I64, I8, V128};

pub(crate) trait PackedShiftCount {
    fn read_count(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I64>, BuildError>;
}

impl PackedShiftCount for MmxLocation {
    fn read_count(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I64>, BuildError> {
        self.read(execution)
    }
}

impl PackedShiftCount for XmmLocation {
    fn read_count(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I64>, BuildError> {
        // Only the low qword supplies the count, but m128 still requires a full aligned read.
        Ok(self
            .read::<V128>(execution, VectorAlignment::Aligned)?
            .extract_lane(0))
    }
}

impl PackedShiftCount for Input<I8> {
    fn read_count(self, execution: &mut ExecutionBuilder<'_, '_>) -> Result<Val<I64>, BuildError> {
        Ok(self.read(execution)?.unsigned().extend())
    }
}
