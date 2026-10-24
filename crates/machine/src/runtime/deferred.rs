use crate::{CpuContext, HookResult};
use std::{future::Future, pin::Pin};

/// Erases the handler while allowing its future to borrow the stopped CPU.
pub(crate) trait DeferredCall<T> {
    fn run<'a>(
        self: Box<Self>,
        call: &'a mut CpuContext<'_, T>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + 'a>>;
}

impl<T, F> DeferredCall<T> for F
where
    F: AsyncFnOnce(&mut CpuContext<'_, T>) -> HookResult + 'static,
{
    fn run<'a>(
        self: Box<Self>,
        call: &'a mut CpuContext<'_, T>,
    ) -> Pin<Box<dyn Future<Output = HookResult> + 'a>> {
        Box::pin(async move { (*self)(call).await })
    }
}
