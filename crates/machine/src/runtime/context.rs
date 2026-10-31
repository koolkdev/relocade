//! Convenience access for the embedding while the runner is stopped.

use crate::{CpuStateAccess, IoBus, MemoryView};

/// Borrowed convenience access to a stopped runner's resources.
/// The fields borrow separate handles and apply writes immediately. A context
/// excludes another mutable borrow of the runner; retain `cpu_state()`, `memory()`
/// and I/O handles instead when registering asynchronous services.
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
