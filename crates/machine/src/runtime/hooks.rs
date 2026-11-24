use futures_util::future::LocalBoxFuture;
use relocade_compiler::{Func, Program};
use relocade_x86::{abi, ExecutionExitValues, HostExit};
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

/// Behavior installed at a dispatch key. Real16 uses packed CS:IP keys.
/// Each hook can be replaced independently. [`Hook::host`] supplies an async
/// Rust handler. [`Hook::block`] builds or imports a Wasm execution block.
///
/// ```
/// use relocade_machine::{CpuStateAccess, Hook, Memory, ExitHandle};
/// fn service(cpu: CpuStateAccess, memory: Memory, exit: ExitHandle) -> Hook {
///     Hook::host(0xf000_0100, async move || {
///         let value = std::future::ready(42).await;
///         cpu.set_ax(value);
///         memory.view().write_u16(0x200, value).unwrap();
///         exit.request();
///     })
/// }
/// ```
///
pub struct Hook {
    pub(super) key: u32,
    pub(super) implementation: Implementation,
}

pub(super) enum Implementation {
    Host(Rc<dyn Handler>),
    Block(Box<dyn FnOnce(&mut Program) -> Func>),
}

impl Hook {
    /// Supplies an ordinary async handler that captures CPU state, memory, I/O and
    /// application resources once at registration. No Wasm runtime borrow or access
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
    pub fn host<F>(key: u32, handler: F) -> Self
    where
        F: AsyncFn() + 'static,
    {
        Self {
            key,
            implementation: Implementation::Host(Rc::new(handler)),
        }
    }

    /// Builds the three dispatch entries of a resumable service. The environment
    /// supplies their keys, sets the live IP to the resume position before an
    /// await and points it at the guest return instruction on normal completion.
    pub fn resumable<F>(entry: u32, resume: u32, completion: u32, handler: F) -> [Self; 3]
    where
        F: AsyncFn() + 'static,
    {
        [
            Self::host(entry, handler),
            Self::block(resume, |program| {
                program
                    .function::<abi::ExecutionSignature>(|body, ()| {
                        body.return_(ExecutionExitValues::host(HostExit::Resume))
                    })
                    .into()
            }),
            Self::block(completion, |program| {
                program.import_function(abi::INTERPRET)
            }),
        ]
    }

    /// Builds or imports a `() -> ExecutionExit` function during installation.
    /// Builders share one compiler Program for the batch. Function signatures
    /// are checked before publication. Use guest dispatch to observe replacement
    /// at another key. Callers must replace directly linked dependent code too.
    pub fn block(key: u32, build: impl FnOnce(&mut Program) -> Func + 'static) -> Self {
        Self {
            key,
            implementation: Implementation::Block(Box::new(build)),
        }
    }
}
