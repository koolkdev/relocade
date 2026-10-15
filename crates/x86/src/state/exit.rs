use wasm86_compiler::{BlockBuilder, BuildError, Val, I32, I64, I8};

use crate::exception::{Exception, ExceptionVector};

const DIVIDE_ERROR: u64 = 1 << 48;
const GENERAL_PROTECTION: u64 = 2 << 48;
const PAGE_FAULT: u64 = 4 << 48;
const UNSUPPORTED_INSTRUCTION: u64 = 8 << 48;
const STACK_FAULT: u64 = 16 << 48;
const SEGMENT_NOT_PRESENT: u64 = 32 << 48;
const BOUND_RANGE_EXCEEDED: u64 = 64 << 48;
const INVALID_OPCODE: u64 = 128 << 48;
const FLOATING_POINT: u64 = 256 << 48;
const SIMD_FLOATING_POINT: u64 = 512 << 48;
const BUDGET_EXHAUSTED: u64 = 1 << 63;
const CONTEXT_CHANGED: u64 = 1027 << 48;
const HOST_YIELD: u64 = 1025 << 48;

/// An exit from a generated execution entry.
///
/// Generated entries can also return values supplied by `dispatch` or `interpret`.
/// Distinguish host-defined callback results from execution exits before decoding.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionExit {
    /// The instruction budget was consumed at a completed block boundary.
    BudgetExhausted,
    /// Execution must resume under a compatible context before another entry.
    ContextChanged,
    /// A completed host call yielded. The next entry resumes its return without
    /// repeating host effects unless execution was explicitly restarted.
    HostYield {
        address: u32,
    },
    Exception(Exception),
    /// An encoding or execution path outside the implemented instruction subset.
    UnsupportedInstruction {
        eip: u32,
        opcode: u8,
    },
}

impl ExecutionExit {
    /// Decodes the documented return word, rejecting unknown tags and nonzero
    /// reserved fields. A host-defined `dispatch` or `interpret` result may also
    /// match a valid exit word. The embedding must distinguish such results;
    /// neither `Some` nor `None` identifies whether guest execution produced it.
    pub fn from_bits(bits: u64) -> Option<Self> {
        let tag = bits & 0xffff_0000_0000_0000;
        let payload = ((bits >> 32) & 0xffff) as u32;
        let address = bits as u32;
        let exception = match tag {
            BUDGET_EXHAUSTED if payload == 0 && address == 0 => {
                return Some(Self::BudgetExhausted);
            }
            CONTEXT_CHANGED if payload == 0 && address == 0 => return Some(Self::ContextChanged),
            HOST_YIELD if payload == 0 => return Some(Self::HostYield { address }),
            DIVIDE_ERROR if payload == 0 && address == 0 => Exception::DivideError,
            BOUND_RANGE_EXCEEDED if payload == 0 && address == 0 => Exception::BoundRangeExceeded,
            INVALID_OPCODE if payload == 0 && address == 0 => Exception::InvalidOpcode,
            FLOATING_POINT if payload == 0 && address == 0 => Exception::FloatingPoint,
            SIMD_FLOATING_POINT if payload == 0 && address == 0 => Exception::SimdFloatingPoint,
            GENERAL_PROTECTION if address == 0 => Exception::GeneralProtection {
                error_code: payload,
            },
            STACK_FAULT if address == 0 => Exception::StackFault {
                error_code: payload,
            },
            SEGMENT_NOT_PRESENT if address == 0 => Exception::SegmentNotPresent {
                error_code: payload,
            },
            PAGE_FAULT => Exception::PageFault {
                linear_address: address,
                error_code: payload,
            },
            UNSUPPORTED_INSTRUCTION if payload <= u32::from(u8::MAX) => {
                return Some(Self::UnsupportedInstruction {
                    eip: address,
                    opcode: payload as u8,
                });
            }
            _ => return None,
        };
        Some(Self::Exception(exception))
    }
}

/// CPU state must already describe the completed block.
pub(crate) fn budget_exhausted(body: BlockBuilder<'_>) -> Result<(), BuildError> {
    body.return_(BUDGET_EXHAUSTED)
}

/// The callable continuation must be retained before yielding to the host.
pub(crate) fn host_yield(body: BlockBuilder<'_>, address: u32) -> Result<(), BuildError> {
    body.return_(HOST_YIELD | u64::from(address))
}

/// Delivers an exception through the host ABI. CPU state must already describe
/// its restart boundary. These host tags are not architectural vector numbers.
pub(crate) fn exception(
    body: BlockBuilder<'_>,
    exception: Exception<Val<I32>>,
) -> Result<(), BuildError> {
    let kind = match exception.vector() {
        ExceptionVector::DivideError => DIVIDE_ERROR,
        ExceptionVector::BoundRangeExceeded => BOUND_RANGE_EXCEEDED,
        ExceptionVector::InvalidOpcode => INVALID_OPCODE,
        ExceptionVector::SegmentNotPresent => SEGMENT_NOT_PRESENT,
        ExceptionVector::StackFault => STACK_FAULT,
        ExceptionVector::GeneralProtection => GENERAL_PROTECTION,
        ExceptionVector::PageFault => PAGE_FAULT,
        ExceptionVector::FloatingPoint => FLOATING_POINT,
        ExceptionVector::SimdFloatingPoint => SIMD_FLOATING_POINT,
    };
    let payload: Val<I64> = match exception {
        Exception::DivideError
        | Exception::BoundRangeExceeded
        | Exception::InvalidOpcode
        | Exception::FloatingPoint
        | Exception::SimdFloatingPoint => 0.into(),
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
    body: BlockBuilder<'_>,
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
