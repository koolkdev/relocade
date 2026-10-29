//! Device-owned requests accepted at safe guest execution boundaries.

use std::{
    cell::RefCell,
    rc::Rc,
    task::{Context, Poll},
};

/// Supplies maskable interrupt vectors to one CPU.
///
/// A device or interrupt controller owns pending requests, priority, masking and
/// acknowledgement policy. The CPU polls only when real-mode interrupt entry is
/// permitted by IF. `Ready(vector)` acknowledges one
/// request; `Pending` must arrange a wake when a request becomes available.
///
/// Requests are considered at instruction-quantum boundaries. Suspended borrowed
/// host services finish before another boundary can be considered.
/// Polling never reenters guest execution.
/// Keep this source on the execution owner, alongside its devices.
pub trait InterruptSource {
    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8>;
}

impl<S: InterruptSource + ?Sized> InterruptSource for Rc<RefCell<S>> {
    fn poll_interrupt(&mut self, cx: &mut Context<'_>) -> Poll<u8> {
        self.borrow_mut().poll_interrupt(cx)
    }
}
