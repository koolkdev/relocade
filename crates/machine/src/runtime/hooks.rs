use relocade_x86::ExecutionExit;

use super::CallContext;
use futures_util::future::LocalBoxFuture;
use std::rc::Rc;

pub(super) trait Handler<T> {
    fn call(self: Rc<Self>, context: CallContext<T>) -> LocalBoxFuture<'static, HookResult>;
}

impl<T: 'static, F> Handler<T> for F
where
    F: AsyncFn(CallContext<T>) -> HookResult + 'static,
{
    fn call(self: Rc<Self>, context: CallContext<T>) -> LocalBoxFuture<'static, HookResult> {
        Box::pin(async move { self(context).await })
    }
}

/// Result of a completed host call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HookResult {
    /// Resume guest dispatch at the continuation set by the handler.
    Continue,
    /// Return an execution exit to the embedding. `HostExit::Call`,
    /// `HostExit::Resume` and `InterruptInhibited` are reserved for execution entries.
    Exit(ExecutionExit),
}

/// A host service's entry and resume labels in an environment-owned guest stub.
/// `T` is the machine's environment state, borrowed through the context.
///
/// ```
/// use relocade_machine::{CallContext, Hook, HookResult};
/// use relocade_x86::{ExecutionExit, HostExit};
///
/// async fn terminate(call: CallContext<Option<u8>>) -> HookResult {
///     call.with(|context| *context.state = Some(context.cpu.al()));
///     HookResult::Exit(ExecutionExit::Host(HostExit::Return))
/// }
/// let hook = Hook::new(0xf0100, 0xf0101, terminate);
/// ```
pub struct Hook<T = ()> {
    pub(super) address: u32,
    pub(super) service: Service<T>,
}

pub(super) struct Service<T> {
    pub resume: u32,
    pub handler: Rc<dyn Handler<T>>,
}

impl<T: 'static> Hook<T> {
    /// Registers distinct linear entry and resume addresses in the same guest stub.
    /// Entry creates an invocation and moves live EIP to `resume`, in the current
    /// CS, before polling its future. Keep that code position while awaiting;
    /// completion selects the environment's return sequence. The resume offset
    /// must fit the CS used to enter the service, including guest aliases.
    /// [`super::real_mode_interrupt::hook`] supplies the BIOS/DOS stub convention.
    ///
    /// Pending awaits allow IRQs according to live IF. IRQ entry naturally saves
    /// the service's resume position. Returning there resumes the top invocation
    /// of this service; it never creates a new one or restores CPU state.
    /// Ready awaits continue synchronously. Futures and locals survive interrupts,
    /// budget exits and waiter changes. Handlers and captures need not be Send or Sync.
    pub fn new<F>(address: u32, resume: u32, handler: F) -> Self
    where
        F: AsyncFn(CallContext<T>) -> HookResult + 'static,
    {
        assert_ne!(address, resume, "service entry and resume must differ");
        Self {
            address,
            service: Service {
                resume,
                handler: Rc::new(handler),
            },
        }
    }
}
