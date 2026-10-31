//! Invocation capabilities and independently retained service resources.

mod scope;
pub use scope::{EnvironmentAccess, InvocationAccess};
pub(super) use scope::{InvocationId, Scope};

use super::machine::Machine;
use crate::{CpuView, IoBus, MemoryBus};
use std::rc::Rc;

/// Owned CPU and environment access, with retained resources for an asynchronous
/// [`super::Hook::new`].
///
/// Ordinary register operations use `call.cpu.ah()` and `call.cpu.set_ax(value)`.
/// The runner lends storage once per future poll. Each operation validates the
/// invocation, while the unique handle's Rust borrows enforce exclusive writes.
/// No backend or CPU borrow is retained between polls.
///
/// Memory and I/O are retained independently and remain available across awaits.
/// Capture service handles from `call.state` for asynchronous work. Environment
/// callbacks borrow only the environment; [`CpuView::with`] batches only CPU work.
///
/// ```
/// # use relocade_machine::{CallContext, MemoryError};
/// async fn read_input(mut call: CallContext, input: impl std::future::Future<Output = u16>)
///     -> Result<(), MemoryError>
/// {
///     let mut memory = call.memory.view();
///     let value = input.await;
///     call.cpu.set_ax(value);
///     memory.write_u16(0x200, value)
/// }
/// ```
///
/// Environment references cannot escape or cross an await:
/// ```compile_fail
/// # use relocade_machine::CallContext;
/// async fn retain_state(mut call: CallContext<u32>) {
///     let state = call.state.with_mut(|state| state);
///     std::future::pending::<()>().await;
///     *state = 1;
/// }
/// ```
/// ```compile_fail
/// # use relocade_machine::CallContext;
/// async fn retain_state(mut call: CallContext<u32>) {
///     call.state.with_mut(|state| async move { *state = 1 }).await;
/// }
/// ```
///
/// Rust excludes another access while a mutable environment callback is active:
/// ```compile_fail
/// # use relocade_machine::CallContext;
/// fn overlap(mut call: CallContext<u32>) {
///     call.state.with_mut(|state| {
///         call.state.with(|other| assert_eq!(*state, *other));
///     });
/// }
/// ```
pub struct CallContext<T = ()> {
    pub cpu: CpuView<InvocationAccess<T>>,
    pub state: EnvironmentAccess<T>,
    pub memory: MemoryBus,
    pub io: IoBus,
}

impl<T> CallContext<T> {
    pub(super) fn new(scope: &Rc<Scope<T>>, machine: &Machine<T>) -> (InvocationId, Self) {
        let access = scope.invocation();
        (
            access.id,
            Self {
                cpu: CpuView::new(machine.bindings.config.execution_target(), access.cpu),
                state: access.state,
                memory: machine.bindings.guest.clone().with_io(machine.io.clone()),
                io: machine.io.clone(),
            },
        )
    }
}
