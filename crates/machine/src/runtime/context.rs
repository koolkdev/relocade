//! Borrowed access to the machine executing a host service.

use crate::{CpuView, IoBus, MemoryAccess, MemoryView};

/// Exclusive access to a stopped machine and its environment.
/// CPU and memory views borrow live storage and apply writes immediately.
/// The fields borrow separate resources and can be used together, including
/// while borrowing environment state. Borrow a field instead of the whole context.
/// A view can cross an ordinary await, keeping its machine borrow until it ends.
/// Contexts cannot escape their handler or the borrow of [`super::Cpu`].
/// Drop the context before resuming guest execution or installing modules.
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
pub struct CpuContext<'a, T = ()> {
    /// Live CPU registers, independent of guest memory and environment state.
    pub cpu: CpuView<MemoryAccess<'a>>,
    /// Guest memory through the machine's physical map.
    pub memory: MemoryView<'a>,
    /// The same device bus used by guest port and MMIO instructions.
    pub io: &'a IoBus,
    /// The environment state owned by this machine.
    pub state: &'a mut T,
}
