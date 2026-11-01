use futures_util::future::LocalBoxFuture;
use relocade_x86::ExecutionExit;
use std::{
    future::Future,
    rc::Rc,
    task::{Context, Poll},
};

pub(super) enum Started<'call> {
    Ready(HookResult),
    Pending(LocalBoxFuture<'call, HookResult>),
}

pub(super) trait Handler {
    fn start(&self, cx: &mut Context<'_>) -> Started<'_>;
}

impl<F> Handler for F
where
    F: AsyncFn() -> HookResult,
{
    fn start(&self, cx: &mut Context<'_>) -> Started<'_> {
        // Pin before the first poll: locals may already borrow one another when
        // this future first suspends. Erase its type only if it remains pending.
        let mut future = Box::pin(self());
        match future.as_mut().poll(cx) {
            Poll::Ready(result) => Started::Ready(result),
            Poll::Pending => Started::Pending(future),
        }
    }
}

/// Result of a completed host service.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookResult {
    /// Dispatch the completion position registered with the service. This changes
    /// only EIP, preserving the guest's current CS alias and all other CPU state.
    Complete,
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return an execution exit to the run loop. `HostExit::Call` and
    /// `HostExit::Resume` are reserved for generated entries.
    Exit(ExecutionExit),
}

/// A host service reached at a guest entry. Its ordinary async handler captures
/// CPU state, memory, I/O and application resources once at registration.
/// No backend borrow or access binding is held across a pending await.
/// Restore temporary CPU changes on normal completion, explicitly after the await.
/// Destructors release resources on abandonment; they must not restore an old
/// invocation's CPU state into the currently executing guest or another call.
///
/// ```
/// use relocade_machine::{CpuStateAccess, Hook, HookResult, Memory};
/// use relocade_x86::{ExecutionExit, HostExit};
/// fn service(cpu: CpuStateAccess, memory: Memory) -> Hook {
///     Hook::new(0xf0100, async move || {
///         let value = std::future::ready(42).await;
///         cpu.set_ax(value);
///         memory.view().write_u16(0x200, value).unwrap();
///         HookResult::Exit(ExecutionExit::Host(HostExit::Return))
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
    /// On Continue, the handler must set the guest continuation in CPU state.
    /// Exit returns its execution exit without changing that state.
    /// Interrupt/ABI return conventions belong to the handler's environment.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    /// Add [`Self::with_continuations`] to permit guest IRQ execution while a
    /// future waits. A bare hook has no guest resume position.
    pub fn new<F>(address: u32, handler: F) -> Self
    where
        F: AsyncFn() -> HookResult + 'static,
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
    /// when the handler returns [`HookResult::Complete`]. Registration does not
    /// change guest bytes. The three positions must be distinct.
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
