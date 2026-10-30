//! Scoped access to the stopped machine.

use crate::{CpuView, IoBus, MemoryView};

/// Exclusive access to a stopped machine and its environment.
/// CPU and memory views borrow live storage and apply writes immediately.
/// The fields borrow separate resources and can be used together, including
/// while borrowing environment state. Borrow a field instead of the whole context.
/// Views supplied by [`super::Cpu::with`] and [`super::CallContext::with`] stay
/// within their synchronous callback and cannot cross an await. Drop all views
/// before resuming guest execution or installing modules.
///
/// A view cannot escape a synchronous embedding operation:
/// ```compile_fail
/// use relocade_machine::Cpu;
/// async fn overlap(cpu: &mut Cpu) {
///     cpu.with(|context| async move {
///         std::future::pending::<()>().await;
///         context.cpu.ax()
///     }).await;
/// }
/// ```
pub struct CpuContext<'a, T = ()> {
    /// Live CPU registers, independent of guest memory and environment state.
    pub cpu: CpuView<relocade_wasm_runtime::MemoryAccess<'a>>,
    /// Guest memory through the machine's physical map.
    pub memory: MemoryView<'a>,
    /// The same device bus used by guest port and MMIO instructions.
    pub io: &'a IoBus,
    /// The environment state owned by this machine.
    pub state: &'a mut T,
}
