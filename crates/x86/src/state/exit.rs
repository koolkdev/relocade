use crate::exception::{Exception, ExceptionValues};
use relocade_compiler::{BlockBuilder, BuildError, Val, I32, I8};

/// A return from a generated execution entry.
///
/// Every generated execution entry shares this Rust ABI.
#[derive(
    relocade_wasm_bindings::WasmAbi,
    relocade_compiler::abi::CompilerValue,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum ExecutionExit {
    /// A host entry or handler requested control outside guest execution.
    Host(HostExit),
    /// The instruction budget was consumed at a completed block boundary.
    BudgetExhausted,
    Exception(Exception),
    /// An encoding or execution path outside the implemented instruction subset.
    UnsupportedInstruction {
        eip: u32,
        opcode: u8,
    },
}

/// Host control flow carried by [`ExecutionExit::Host`].
#[derive(
    relocade_wasm_bindings::WasmAbi,
    relocade_compiler::abi::CompilerValue,
    Clone,
    Copy,
    Debug,
    Eq,
    PartialEq,
)]
pub enum HostExit {
    /// A host-call function requested a return to the embedding.
    /// The embedding owns the reason, such as process termination or a service error.
    Return,
    /// A generated entry requests the handler at this guest linear address.
    /// CPU state is published; the runtime consumes this exit before resuming.
    /// Reserved for execution entries, not handler results.
    Call { address: u32 },
}

/// CPU state must already describe the completed block.
pub(crate) fn budget_exhausted(body: BlockBuilder<'_>) -> Result<(), BuildError> {
    body.return_(ExecutionExitValues::budget_exhausted())
}

/// CPU state must already describe the exception's restart boundary.
pub(crate) fn exception(
    body: BlockBuilder<'_>,
    exception: impl Into<ExceptionValues>,
) -> Result<(), BuildError> {
    body.return_(ExecutionExitValues::exception(exception))
}

pub(crate) fn unsupported(
    body: BlockBuilder<'_>,
    address: &Val<I32>,
    opcode: impl Into<Val<I8>>,
) -> Result<(), BuildError> {
    body.return_(ExecutionExitValues::unsupported_instruction(
        address, opcode,
    ))
}

#[cfg(test)]
mod tests;
