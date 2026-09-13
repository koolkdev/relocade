use wasm86_compiler::{BuildError, FunctionBuilder, Val, I32, I64, I8};

use crate::exception::{Exception, ExceptionVector};

const DIVIDE_ERROR: u64 = 1 << 48;
const GENERAL_PROTECTION: u64 = 2 << 48;
const PAGE_FAULT: u64 = 4 << 48;
const UNSUPPORTED_INSTRUCTION: u64 = 8 << 48;
const STACK_FAULT: u64 = 16 << 48;

/// Delivers an exception through the host ABI. CPU state must already describe
/// its restart boundary. These host tags are not architectural vector numbers.
pub(crate) fn exception(body: FunctionBuilder<'_>, exception: Exception) -> Result<(), BuildError> {
    let kind = match exception.vector() {
        ExceptionVector::DivideError => DIVIDE_ERROR,
        ExceptionVector::StackFault => STACK_FAULT,
        ExceptionVector::GeneralProtection => GENERAL_PROTECTION,
        ExceptionVector::PageFault => PAGE_FAULT,
    };
    let payload: Val<I64> = match exception {
        Exception::DivideError => 0.into(),
        Exception::GeneralProtection { error_code } | Exception::StackFault { error_code } => {
            error_code.unsigned().extend::<I64>().shl(32)
        }
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
    opcode: &Val<I8>,
) -> Result<(), BuildError> {
    body.return_(
        address
            .unsigned()
            .extend::<I64>()
            .or(opcode.unsigned().extend::<I64>().shl(32))
            .or(UNSUPPORTED_INSTRUCTION),
    )
}

#[cfg(test)]
mod tests;
