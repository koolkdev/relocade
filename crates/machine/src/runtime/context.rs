//! Borrowed resource access while guest instructions are not executing.

use super::execution::{GuestCall, GuestCalls};
use crate::{CpuStateAccess, FarPointer, IoBus, MemoryView};

/// Resource access for embeddings and host services.
/// The fields borrow separate handles and apply writes immediately. A context
/// obtained from a runner excludes another mutable borrow of that runner.
/// Registered interrupt handlers receive these resources together with access
/// to [`Self::call_interrupt`] and [`Self::call_far`] for awaiting guest code.
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
    /// Clone this handle to retain synchronous I/O beyond the context's lifetime,
    /// as with [`super::Cpu::io`]. Cloning retains the runtime without borrowing
    /// the CPU controller.
    pub io: &'a IoBus,
    pub(super) guest_calls: Option<&'a GuestCalls>,
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
        self.invoke(GuestCall::Interrupt(vector)).await;
    }

    /// Calls a real-mode guest procedure and awaits its RETF through the current
    /// service's resume entry. Words are supplied in push order, followed by CS
    /// and IP. Entry checks the complete frame before writing it and leaves FLAGS
    /// unchanged. The frame must fit contiguously within SS, including arguments.
    /// After RETF, this method discards the argument words from the live
    /// stack. The callee must not pop them. Register preservation belongs to the caller.
    ///
    /// Uses the same nesting, fault recovery and cancellation lifecycle as
    /// [`Self::call_interrupt`], including device IRQs while the callee runs.
    /// Panics outside a registered interrupt handler or outside real mode.
    /// Panics if the argument bytes exceed the 16-bit stack's addressable size.
    pub async fn call_far(&mut self, target: FarPointer, arguments: &[u16]) {
        let stack_bytes = u16::try_from(size_of_val(arguments))
            .expect("far-call arguments must fit a 16-bit stack");
        self.invoke(GuestCall::Far {
            target,
            arguments: arguments.into(),
        })
        .await;
        // RETF consumed only the return address. Cleanup uses the current stack,
        // including any entry-fault repair, and never runs on abandonment.
        self.cpu.set_sp(self.cpu.sp().wrapping_add(stack_bytes));
    }

    async fn invoke(&mut self, target: GuestCall) {
        assert_eq!(
            self.cpu.execution_target().profile,
            relocade_x86::ExecutionProfile::Real16,
            "guest calls require real mode"
        );
        self.guest_calls
            .expect("guest code must be called inside an interrupt handler")
            .call(target)
            .await;
    }
}
