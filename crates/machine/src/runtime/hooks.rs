use futures_util::future::LocalBoxFuture;
use relocade_compiler::{Func, Program};
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

/// Behavior installed at a guest execution address. Each hook is independently
/// replaceable. [`Hook::host`] supplies an ordinary async Rust handler;
/// [`Hook::block`] builds or imports a Wasm execution block.
///
/// ```
/// use relocade_machine::{CpuStateAccess, Hook, Memory, ExitHandle};
/// fn service(cpu: CpuStateAccess, memory: Memory, exit: ExitHandle) -> Hook {
///     Hook::host(0xf0100, async move || {
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
    pub(super) implementation: Implementation,
}

pub(super) enum Implementation {
    Host(Rc<dyn Handler>),
    Block(Box<dyn FnOnce(&mut Program) -> Func>),
}

impl Hook {
    /// Supplies an ordinary async handler that captures CPU state, memory, I/O and
    /// application resources once at registration. No backend borrow or access
    /// binding is held across a pending await. Restore temporary CPU changes on
    /// normal completion, explicitly after the await. Destructors release resources
    /// on abandonment; they must not restore an old invocation's CPU state into the
    /// currently executing guest or another call.
    ///
    /// Before suspending with live IF enabled, establish a guest continuation that
    /// emits `HostExit::Resume`: IRQ entry saves the live CS:IP, and resumption polls
    /// the retained future without restoring CPU state. Interrupt adapters establish
    /// that position before invoking the handler. Raw hooks own their continuation.
    ///
    /// Normal completion continues guest execution at the live CPU position.
    /// Interrupt/ABI return conventions belong to the handler's environment;
    /// [`super::real_mode_interrupt::hook`] applies the interrupt return convention.
    /// A captured [`super::ExitHandle`] can request that the current run end.
    /// Handlers run in Rust after guest Wasm returns. Captures and environment
    /// data stay on the CPU's thread and do not need Send or Sync.
    pub fn host<F>(address: u32, handler: F) -> Self
    where
        F: AsyncFn() + 'static,
    {
        Self {
            address,
            implementation: Implementation::Host(Rc::new(handler)),
        }
    }

    /// Builds or imports a `() -> ExecutionExit` function during installation.
    /// Builders share one compiler Program for the batch. Function signatures
    /// are checked before publication. Use guest dispatch to observe replacement
    /// at another address. Callers must replace directly linked dependent code too.
    pub fn block(address: u32, build: impl FnOnce(&mut Program) -> Func + 'static) -> Self {
        Self {
            address,
            implementation: Implementation::Block(Box::new(build)),
        }
    }
}
