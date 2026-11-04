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
    Exception(Exception),
    /// An encoding or execution path outside the implemented instruction subset.
    UnsupportedInstruction {
        eip: u32,
        opcode: u8,
    },
    /// External interrupt entry completed without retiring an instruction.
    /// CPU state identifies the ISR; the caller may now dispatch guest execution.
    InterruptEntered,
    /// STI or an SS load completed with interrupt delivery inhibited. Execute
    /// the successor through interpreter STEP before polling IRQs or returning
    /// at an execution checkpoint.
    InterruptInhibited,
    /// An execution interrupt was requested at a completed execution boundary.
    /// CPU state is published. This neither delivers a guest IRQ nor consumes
    /// the request; the embedding acknowledges it before examining its work.
    Interrupted,
    /// Interpreter STEP completed one instruction without dispatching its successor.
    /// CPU state and retirement count are published.
    StepComplete,
    /// Real-mode POPF or IRET enabled interrupts. CPU state is published;
    /// poll pending device IRQs before continuing guest execution.
    InterruptsEnabled,
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
    /// A generated entry requests a runtime-owned host service by ID.
    /// The ID is independent of the guest entry address.
    /// CPU state is published; the runtime consumes this exit before resuming.
    /// Reserved for execution entries, not handler results.
    Call { service_id: u32 },
    /// A guest resume entry reactivates the top suspended host invocation.
    /// The strictly nested execution stack supplies the invocation; no ID is needed.
    Resume,
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
