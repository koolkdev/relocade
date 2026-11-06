//! Borrowed resource access while guest instructions are not executing.

use super::execution::InterruptCalls;
use crate::{CpuStateAccess, IoBus, MemoryView};

/// Resource access for embeddings and host services.
/// The fields borrow separate handles and apply writes immediately. A context
/// obtained from a runner excludes another mutable borrow of that runner.
/// Registered interrupt handlers receive these resources together with access
/// to [`Self::call_interrupt`] for awaiting other interrupt handlers.
///
/// A live view excludes another run:
/// ```compile_fail
/// use relocade_machine::Cpu;
/// async fn overlap(cpu: &mut Cpu) {
///     let mut context = cpu.context();
///     let view = &context.cpu;
///     cpu.run().await;
///     let _ = view.ax();
/// }
/// ```
pub struct CpuContext<'a> {
    /// Live CPU state, independent of guest memory and environment state.
    pub cpu: &'a CpuStateAccess,
    /// Guest memory through the machine's physical map.
    pub memory: MemoryView<'a>,
    /// The same device bus used by guest port and MMIO instructions.
    pub io: &'a IoBus,
    pub(super) interrupt_calls: Option<&'a InterruptCalls>,
}

impl CpuContext<'_> {
    /// Calls the current real-mode interrupt vector and awaits its handler's return.
    /// The vector may lead to x86 code or another registered Rust handler.
    ///
    /// Available inside a registered interrupt handler. Entry uses the live IVT
    /// and pushes FLAGS, CS and IP through the normal x86 interrupt machinery,
    /// regardless of IF. IRET resumes this handler at the await.
    /// Register preservation and temporary IF changes belong to the caller.
    ///
    /// Guest faults and execution checkpoints return from `Cpu::run` while
    /// retaining the caller. Dropping a run waiter does not cancel the invocation.
    /// Exiting the CPU or explicitly abandoning suspended calls drops the waiting
    /// handler without completing it. Canceling before entry discards the request.
    ///
    /// # Panics
    /// Panics outside a registered interrupt handler or outside real mode.
    pub async fn call_interrupt(&mut self, vector: u8) {
        assert_eq!(
            self.cpu.execution_target().profile,
            relocade_x86::ExecutionProfile::Real16,
            "interrupt calls require real mode"
        );
        self.interrupt_calls
            .expect("interrupts must be called inside an interrupt handler")
            .call(vector)
            .await;
    }
}
