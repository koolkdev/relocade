//! Scoped resources lent by a caller for one synchronous guest invocation.

use crate::WasmContext;
use std::any::Any;

/// Resources that can be lent to Wasm callbacks for one call. Native resources
/// must be `Sync` because a scoped caller can move to another thread. Browser
/// resources remain on their JavaScript thread.
#[cfg(not(target_arch = "wasm32"))]
pub trait ExecutionData: Any + Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Any + Sync> ExecutionData for T {}
#[cfg(target_arch = "wasm32")]
pub trait ExecutionData: Any {}
#[cfg(target_arch = "wasm32")]
impl<T: Any> ExecutionData for T {}

/// A callback's engine borrow, with access to the resources lent by
/// [`crate::Function::call_with`]. The closure cannot retain either borrow.
///
/// ```compile_fail
/// use relocade_wasm_runtime::WasmCallback;
/// fn escape<C: WasmCallback>(caller: &mut C) -> &'static Vec<u8> {
///     caller.with_execution_data::<Vec<u8>, _>(|_, resources| resources)
/// }
/// ```
pub trait WasmCallback: WasmContext {
    fn with_execution_data<D: ExecutionData, R>(
        &mut self,
        callback: impl for<'a> FnOnce(&'a mut Self, &'a D) -> R,
    ) -> R;
}

#[cfg(not(target_arch = "wasm32"))]
type Data = dyn Any + Sync;
#[cfg(target_arch = "wasm32")]
type Data = dyn Any;

#[derive(Default)]
pub(crate) struct ScopedData(Option<&'static Data>);

pub(crate) trait ScopedContext {
    fn scoped_data(&mut self) -> &mut ScopedData;
}

/// Only backend typed-call implementations may use this publisher. Passing the
/// root context to arbitrary user code would let it replace the context and
/// detach the live slot from its cleanup guard.
#[allow(unsafe_code)]
pub(crate) fn call_with<C: ScopedContext, D: ExecutionData, R>(
    context: &mut C,
    data: &D,
    call: impl FnOnce(&mut C) -> R,
) -> R {
    let slot = context.scoped_data();
    assert!(slot.0.is_none(), "execution resources already lent");
    let data: &Data = data;
    // SAFETY: Active retains the root context's exclusive borrow and clears the
    // private slot before this synchronous function returns, including unwind.
    // No reference to the slot survives guest execution. Callbacks receive only
    // closure-scoped shared reborrows; native Data is Sync for scoped threads.
    slot.0 = Some(unsafe { std::mem::transmute::<&Data, &'static Data>(data) });

    struct Active<'a, C: ScopedContext>(&'a mut C);
    impl<C: ScopedContext> Drop for Active<'_, C> {
        fn drop(&mut self) {
            self.0.scoped_data().0 = None;
        }
    }
    let active = Active(context);
    call(active.0)
}

pub(crate) fn with_data<C: ScopedContext, D: ExecutionData, R>(
    context: &mut C,
    callback: impl for<'a> FnOnce(&'a mut C, &'a D) -> R,
) -> R {
    let data: &dyn Any = context
        .scoped_data()
        .0
        .expect("callback requires active execution resources");
    let data = data
        .downcast_ref::<D>()
        .expect("callback execution resource type mismatch");
    callback(context, data)
}
