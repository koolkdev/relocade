//! Retains a pending future together with the handler it can borrow.
//!
//! CPU state and other resources are ordinary handler captures. This module
//! bridges only the lifetime of a future borrowing its stable handler owner;
//! it does not bind storage, lend mutable references or grant CPU access.

use super::hooks::{Handler, Started};
use super::HookResult;
use futures_util::future::LocalBoxFuture;
use std::{
    rc::Rc,
    task::{Context, Poll},
};

pub(super) enum CallPoll {
    Ready(HookResult),
    Pending(Call),
}

pub(super) struct Call {
    // Field order is part of the lifetime proof: destroy the future before its
    // captured owner, on completion, abandonment and unwinding alike.
    future: LocalBoxFuture<'static, HookResult>,
    _handler: Rc<dyn Handler>,
}

impl Call {
    #[inline]
    pub fn start(handler: &Rc<dyn Handler>, cx: &mut Context<'_>) -> CallPoll {
        match handler.start(cx) {
            Started::Ready(result) => CallPoll::Ready(result),
            Started::Pending(future) => {
                let owner = handler.clone();
                // SAFETY: start can borrow only this handler. Its Rc allocation
                // stays stable and is retained until after the pinned future is
                // destroyed. Call never exposes the future, moves its contents,
                // or permits replacing the owner independently of it.
                let future = unsafe {
                    std::mem::transmute::<
                        LocalBoxFuture<'_, HookResult>,
                        LocalBoxFuture<'static, HookResult>,
                    >(future)
                };
                CallPoll::Pending(Self {
                    future,
                    _handler: owner,
                })
            }
        }
    }

    pub fn poll(mut self, cx: &mut Context<'_>) -> CallPoll {
        match self.future.as_mut().poll(cx) {
            Poll::Ready(result) => CallPoll::Ready(result),
            Poll::Pending => CallPoll::Pending(self),
        }
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests;
