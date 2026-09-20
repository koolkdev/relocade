use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I64, I8};

use crate::exception::{Exception, ExceptionVector};

const DIVIDE_ERROR: u64 = 1 << 48;
const GENERAL_PROTECTION: u64 = 2 << 48;
const PAGE_FAULT: u64 = 4 << 48;
const UNSUPPORTED_INSTRUCTION: u64 = 8 << 48;
const STACK_FAULT: u64 = 16 << 48;
const SEGMENT_NOT_PRESENT: u64 = 32 << 48;
const BOUND_RANGE_EXCEEDED: u64 = 64 << 48;

/// Delivers an exception through the host ABI. CPU state must already describe
/// its restart boundary. These host tags are not architectural vector numbers.
pub(crate) fn exception(
    body: FunctionBuilder<'_>,
    exception: Exception<Val<I32>>,
) -> Result<(), BuildError> {
    let kind = match exception.vector() {
        ExceptionVector::DivideError => DIVIDE_ERROR,
        ExceptionVector::BoundRangeExceeded => BOUND_RANGE_EXCEEDED,
        ExceptionVector::SegmentNotPresent => SEGMENT_NOT_PRESENT,
        ExceptionVector::StackFault => STACK_FAULT,
        ExceptionVector::GeneralProtection => GENERAL_PROTECTION,
        ExceptionVector::PageFault => PAGE_FAULT,
    };
    let payload: Val<I64> = match exception {
        Exception::DivideError | Exception::BoundRangeExceeded => 0.into(),
        Exception::SegmentNotPresent { error_code }
        | Exception::GeneralProtection { error_code }
        | Exception::StackFault { error_code } => error_code.unsigned().extend::<I64>().shl(32),
        Exception::PageFault {
            linear_address,
            error_code,
        } => linear_address
            .unsigned()
            .extend::<I64>()
            .or(error_code.unsigned().extend::<I64>().shl(32)),
    };
    body.return_(payload.or(kind))
}

pub(crate) fn unsupported(
    body: FunctionBuilder<'_>,
    address: &Val<I32>,
    opcode: impl Into<Val<I8>>,
) -> Result<(), BuildError> {
    body.return_(
        address
            .unsigned()
            .extend::<I64>()
            .or(opcode.into().unsigned().extend::<I64>().shl(32))
            .or(UNSUPPORTED_INSTRUCTION),
    )
}

#[cfg(test)]
mod tests;
