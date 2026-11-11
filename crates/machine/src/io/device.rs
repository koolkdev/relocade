use super::IoContext;
use std::{
    cell::RefCell,
    future::{ready, Future},
    ops::RangeInclusive,
    rc::Rc,
    task::{Context, Poll},
};

/// A synchronous device on the machine's memory and port buses.
/// Addresses are physical bus addresses, after A20 translation for MMIO.
/// Values are little endian. MMIO transfers contain one through eight bytes;
/// port transfers retain their architectural width of one, two or four bytes.
///
/// The execution worker owns device state. Reads can have side effects, so every
/// transfer borrows the device mutably. Callbacks must not reenter guest execution
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

    /// Reads 1, 2 or 4 bytes from a port. Only the low `bytes` bytes of the
    /// result are consumed; unused upper bits need not be cleared.
    fn read_port(&mut self, _context: IoContext<'_>, _port: u16, _bytes: u8) -> u32 {
        u32::MAX
    }

    /// Writes 1, 2 or 4 bytes to a port, with unused upper value bits cleared.
    fn write_port(&mut self, _context: IoContext<'_>, _port: u16, _bytes: u8, _value: u32) {}

    /// Supplies and acknowledges one pending maskable interrupt vector.
    /// Retain requests until polled. Return Pending and register the waker when a
    /// future request should wake a waiting CPU. This callback must not reenter
    /// guest execution. The default never raises interrupts.
    fn poll_interrupt(&mut self, _context: IoContext<'_>, _cx: &mut Context<'_>) -> Poll<u8> {
        Poll::Pending
    }
}

/// The addresses decoded by one device. Memory ranges must also be marked
/// [`relocade_x86::PhysicalMapping::Mmio`] in the machine's physical map.
/// The same connection carries interrupt requests; address ranges may be empty.
pub struct DeviceConnection {
    pub device: Rc<RefCell<dyn Device>>,
    pub mmio: Vec<RangeInclusive<u32>>,
    pub ports: Vec<RangeInclusive<u16>>,
}
