//! Registered service factories and pinned storage for their live invocations.

use super::{CpuContext, HookResult};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll},
};

pub(super) struct StartedCall {
    pub slot: usize,
    pub result: Poll<HookResult>,
}

pub(super) trait Handler<T> {
    fn start(&mut self, context: CpuContext<T>, cx: &mut Context<'_>) -> StartedCall;
    fn poll(&mut self, slot: usize, cx: &mut Context<'_>) -> Poll<HookResult>;
    fn clear(&mut self, slot: usize);
}

pub(super) struct Service<F, Fut> {
    factory: F,
    slots: Vec<Pin<Box<Option<Fut>>>>,
}

impl<F, Fut> Service<F, Fut> {
    pub fn new(factory: F) -> Self {
        Self {
            factory,
            slots: Vec::new(),
        }
    }
}

impl<T, F, Fut> Handler<T> for Service<F, Fut>
where
    F: Fn(CpuContext<T>) -> Fut,
    Fut: Future<Output = HookResult>,
{
    fn start(&mut self, context: CpuContext<T>, cx: &mut Context<'_>) -> StartedCall {
        let slot = self
            .slots
            .iter()
            .position(|slot| slot.is_none())
            .unwrap_or_else(|| {
                self.slots.push(Box::pin(None));
                self.slots.len() - 1
            });
        self.slots[slot].as_mut().set(Some((self.factory)(context)));
        StartedCall {
            slot,
            result: self.poll(slot, cx),
        }
    }

    fn poll(&mut self, slot: usize, cx: &mut Context<'_>) -> Poll<HookResult> {
        self.slots[slot]
            .as_mut()
            .as_pin_mut()
            .expect("active host call")
            .poll(cx)
    }

    fn clear(&mut self, slot: usize) {
        // Drop in place. A future may hold references to its own local variables.
        self.slots[slot].as_mut().set(None);
    }
}

/// Keeps the original registration alive if its address is replaced while waiting.
pub(super) struct PendingCall<T> {
    pub id: u64,
    pub handler: Rc<RefCell<dyn Handler<T>>>,
    pub slot: usize,
}

impl<T> Drop for PendingCall<T> {
    fn drop(&mut self) {
        self.handler.borrow_mut().clear(self.slot);
    }
}
