use super::{Call, HookResult};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

pub(super) type CallFuture = Pin<Box<dyn Future<Output = HookResult>>>;

/// Compatibility for one-shot operations queued by a synchronous hook.
pub(super) trait DeferredCall<T: 'static> {
    fn start(self: Box<Self>, call: Call<T>) -> CallFuture;
}

impl<T: 'static, F, Fut> DeferredCall<T> for F
where
    F: FnOnce(Call<T>) -> Fut + 'static,
    Fut: Future<Output = HookResult> + 'static,
{
    fn start(self: Box<Self>, call: Call<T>) -> CallFuture {
        Box::pin((*self)(call))
    }
}

/// Erases a registered factory, while preserving reusable typed future storage.
pub(super) trait AsyncHandler<T: 'static> {
    fn start(&mut self, call: Call<T>) -> usize;
    fn poll(&mut self, slot: usize, cx: &mut Context<'_>) -> Poll<HookResult>;
    fn clear(&mut self, slot: usize);
}

pub(super) struct Handler<F, Fut> {
    factory: F,
    slots: Vec<Pin<Box<Option<Fut>>>>,
}

impl<F, Fut> Handler<F, Fut> {
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            slots: Vec::new(),
        }
    }
}

impl<T, F, Fut> AsyncHandler<T> for Handler<F, Fut>
where
    T: 'static,
    F: Fn(Call<T>) -> Fut + 'static,
    Fut: Future<Output = HookResult> + 'static,
{
    fn start(&mut self, call: Call<T>) -> usize {
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.is_none())
            .unwrap_or_else(|| {
                self.slots.push(Box::pin(None));
                self.slots.len() - 1
            });
        self.slots[slot].as_mut().set(Some((self.factory)(call)));
        slot
    }

    fn poll(&mut self, slot: usize, cx: &mut Context<'_>) -> Poll<HookResult> {
        self.slots[slot]
            .as_mut()
            .as_pin_mut()
            .expect("active host call")
            .poll(cx)
    }

    fn clear(&mut self, slot: usize) {
        // Drop in place, including futures that hold references to their own locals.
        self.slots[slot].as_mut().set(None);
    }
}

pub(super) enum Invocation {
    Registered { handler: usize, slot: usize },
    Once(CallFuture),
}

pub(super) struct CallFrame {
    pub id: u64,
    pub invocation: Invocation,
}
