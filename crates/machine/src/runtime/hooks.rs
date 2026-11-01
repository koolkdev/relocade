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
///
/// ```
/// use relocade_machine::{CpuStateAccess, Hook, Memory, StopHandle};
/// fn service(cpu: CpuStateAccess, memory: Memory, stop: StopHandle) -> Hook {
///     Hook::new(0xf0100, async move || {
///         let value = std::future::ready(42).await;
///         cpu.set_ax(value);
///         memory.view().write_u16(0x200, value).unwrap();
///         stop.stop();
///     })
/// }
/// ```
///
pub struct Hook {
    pub(super) address: u32,
    pub(super) handler: Rc<dyn Handler>,
    pub(super) continuations: Option<Continuations>,
}

#[derive(Clone, Copy)]
pub(super) struct Continuations {
    pub resume: u32,
    pub complete: u32,
}

impl Hook {
    /// On return, a bare hook uses the continuation set in CPU state by its
    /// handler. With registered continuations, return selects the completion
    /// position instead. A [`super::StopHandle`] request skips completion.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    /// Add [`Self::with_continuations`] to permit guest IRQ execution while a
    /// future waits. A bare hook has no guest resume position.
    pub fn new<F>(address: u32, handler: F) -> Self
    where
        F: AsyncFn() + 'static,
    {
        Self {
            address,
            handler: Rc::new(handler),
            continuations: None,
        }
    }

    /// Supplies linear resume and completion positions in the environment's
    /// existing service code. No global guest address is reserved by the runtime.
    ///
    /// Entry starts a fresh invocation. Resume identifies suspended execution;
    /// it never starts a handler. Complete selects guest code, such as IRET,
    /// when the handler returns. Registration does not change guest bytes.
    /// The three positions must be distinct.
    ///
    /// [`super::real_mode_interrupt::hook`] supplies the BIOS/DOS convention.
    pub fn with_continuations(mut self, resume: u32, complete: u32) -> Self {
        assert_ne!(self.address, resume, "entry and resume must differ");
        assert_ne!(self.address, complete, "entry and completion must differ");
        assert_ne!(resume, complete, "resume and completion must differ");
        self.continuations = Some(Continuations { resume, complete });
        self
    }
}
