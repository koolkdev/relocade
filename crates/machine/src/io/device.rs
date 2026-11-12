use super::IoContext;
use std::{
    cell::RefCell,
    future::{ready, Future},
    rc::Rc,
    task::{Context, Poll},
};

/// An execution-owned device with MMIO, interrupts and optional async activity.
/// MMIO addresses are physical addresses after A20 translation. Values are
/// little endian and transfers contain one through eight bytes.
/// Port operations are installed separately through [`crate::DeviceConnection::bind`];
/// [`crate::DeviceCallbacks`] can adapt methods that borrow this device.
///
/// The execution worker owns device state. MMIO reads can have side effects,
/// so they borrow the device mutably. Callbacks must not reenter guest execution
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

    fn read_mmio(&mut self, _context: IoContext<'_>, _address: u32, _bytes: u8) -> u64 {
        u64::MAX
    }

    fn write_mmio(&mut self, _context: IoContext<'_>, _address: u32, _bytes: u8, _value: u64) {}

    /// Supplies and acknowledges one pending maskable interrupt vector.
    /// Retain requests until polled. Return Pending and register the waker when a
    /// future request should wake a waiting CPU. This callback must not reenter
    /// guest execution. The default never raises interrupts.
    fn poll_interrupt(&mut self, _context: IoContext<'_>, _cx: &mut Context<'_>) -> Poll<u8> {
        Poll::Pending
    }
}
