use super::IoContext;
use std::{
    cell::RefCell,
    future::{ready, Future},
    rc::Rc,
    task::{Context, Poll},
};

/// An execution-owned device with interrupts and optional async activity.
/// Port and MMIO operations are installed through [`crate::DeviceConnection::bind`]
/// and [`crate::DeviceConnection::bind_mmio`]. [`crate::DeviceCallbacks`] can adapt
/// concrete methods that borrow this device.
///
/// The execution worker owns device state. Callbacks must not reenter guest execution
/// or access the same device recursively. Publish owned observations separately
/// when another worker needs to display or inspect device output.
/// Use the supplied [`IoContext`] to forward I/O;
/// host convenience methods acquire a separate stopped-runtime borrow.
pub trait Device {
    /// Optional asynchronous device activity, registered by [`crate::Runner::connect`].
    /// Connection waits for the first poll, which installs this activity's wake
    /// path. Device state must already be initialized before connecting the device.
    /// Release device borrows before awaiting. Continuously ready activities must
    /// cooperate with other work, for example through [`crate::yield_now`].
    fn run(_device: Rc<RefCell<Self>>) -> impl Future<Output = ()>
    where
        Self: Sized,
    {
        ready(())
    }

    /// Supplies and acknowledges one pending maskable interrupt vector.
    /// Retain requests until polled. Return Pending and register the waker when a
    /// future request should wake a waiting CPU. This callback must not reenter
    /// guest execution. The default never raises interrupts.
    fn poll_interrupt(&mut self, _context: IoContext<'_>, _cx: &mut Context<'_>) -> Poll<u8> {
        Poll::Pending
    }
}
