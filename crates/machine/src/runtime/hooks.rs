use futures_util::future::LocalBoxFuture;
use std::{
    future::Future,
    rc::Rc,
    task::{Context, Poll},
};

pub(super) trait Handler {
    fn start(&self, cx: &mut Context<'_>) -> Option<LocalBoxFuture<'_, ()>>;
}

impl<F> Handler for F
where
    F: AsyncFn(),
{
    fn start(&self, cx: &mut Context<'_>) -> Option<LocalBoxFuture<'_, ()>> {
        // Pin before the first poll: locals may already borrow one another when
        // this future first suspends. Erase its type only if it remains pending.
        let mut future = Box::pin(self());
        match future.as_mut().poll(cx) {
            Poll::Ready(()) => None,
            Poll::Pending => Some(future),
        }
    }
}

/// A host service reached at a guest entry. Its ordinary async handler captures
/// CPU state, memory, I/O and application resources once at registration.
/// No backend borrow or access binding is held across a pending await.
/// Restore temporary CPU changes on normal completion, explicitly after the await.
/// Destructors release resources on abandonment; they must not restore an old
/// invocation's CPU state into the currently executing guest or another call.
/// Before suspending with live IF enabled, establish a guest continuation that
/// emits `HostExit::Resume`: IRQ entry saves the live CS:IP, and resumption polls
/// the retained future without restoring CPU state. Interrupt adapters establish
/// that position before invoking the handler. Raw hooks own their continuation.
///
/// ```
/// use relocade_machine::{CpuStateAccess, Hook, Memory, ExitHandle};
/// fn service(cpu: CpuStateAccess, memory: Memory, exit: ExitHandle) -> Hook {
///     Hook::new(0xf0100, async move || {
///         let value = std::future::ready(42).await;
///         cpu.set_ax(value);
///         memory.view().write_u16(0x200, value).unwrap();
///         exit.request();
///     })
/// }
/// ```
///
pub struct Hook {
    pub(super) address: u32,
    pub(super) handler: Rc<dyn Handler>,
}

impl Hook {
    /// Normal completion continues guest execution at the live CPU position.
    /// Interrupt/ABI return conventions belong to the handler's environment;
    /// [`super::real_mode_interrupt::hook`] applies the interrupt return convention.
    /// A captured [`super::ExitHandle`] can request that the current run end.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    pub fn new<F>(address: u32, handler: F) -> Self
    where
        F: AsyncFn() + 'static,
    {
        Self {
            address,
            handler: Rc::new(handler),
        }
    }
}
