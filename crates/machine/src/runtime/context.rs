//! Borrowed resource access while guest instructions are not executing.

use crate::{CpuStateAccess, IoBus, MemoryView};

/// Resource access for embeddings and host services.
/// The fields borrow separate handles and apply writes immediately. A context
/// obtained from a runner excludes another mutable borrow of that runner.
/// Asynchronous services construct their context from retained `cpu_state()`,
/// `memory()` and I/O handles instead.
///
/// A live view excludes another run:
/// ```compile_fail
/// use relocade_machine::Cpu;
/// async fn overlap(cpu: &mut Cpu) {
///     let mut context = cpu.context();
///     let view = &context.cpu;
///     cpu.run(1).await;
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
}
