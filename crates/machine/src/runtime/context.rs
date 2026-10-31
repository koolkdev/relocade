//! Borrowed access for embedding code while the CPU is stopped.

use crate::{CpuStorage, CpuView, IoBus, MemoryView};

/// Exclusive access to a stopped machine.
/// CPU and memory views borrow live storage and apply writes immediately.
/// The fields borrow separate resources and can be used together.
/// Application state is captured by services independently of this context.
/// A view can cross an ordinary await, keeping its machine borrow until it ends.
/// Contexts cannot escape the borrow of [`super::Cpu`]. Host services receive
/// [`super::HookCpu`] and capture their memory and I/O handles separately.
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
pub struct CpuContext<'a> {
    /// Live CPU registers, independent of guest memory and environment state.
    pub cpu: CpuView<CpuStorage<'a>>,
    /// Guest memory through the machine's physical map.
    pub memory: MemoryView<'a>,
    /// The same device bus used by guest port and MMIO instructions.
    pub io: &'a IoBus,
}
