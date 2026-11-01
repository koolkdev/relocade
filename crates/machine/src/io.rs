//! Device connections local to guest execution and its host services.

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
pub trait Device {
    /// Optional asynchronous device activity, registered by [`crate::Runner::connect`].
    /// Release device borrows before awaiting. Continuously ready activities must
    /// cooperate with other work, for example through [`crate::yield_now`].
    fn run(_device: Rc<RefCell<Self>>) -> impl Future<Output = ()>
    where
        Self: Sized,
    {
        ready(())
    }

    fn read_mmio(&mut self, _address: u32, _bytes: u8) -> u64 {
        u64::MAX
    }

    fn write_mmio(&mut self, _address: u32, _bytes: u8, _value: u64) {}

    fn read_port(&mut self, _port: u16, _bytes: u8) -> u32 {
        u32::MAX
    }

    fn write_port(&mut self, _port: u16, _bytes: u8, _value: u32) {}

    /// Supplies and acknowledges one pending maskable interrupt vector.
    /// Retain requests until polled. Return Pending and register the waker when a
    /// future request should wake a waiting CPU. This callback must not reenter
    /// guest execution. The default never raises interrupts.
    fn poll_interrupt(&mut self, _cx: &mut Context<'_>) -> Poll<u8> {
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

/// Immutable routing on one execution worker. Clones retain the same local
/// devices; the bus cannot be sent or shared across threads.
/// Unconnected reads return FF bytes and unconnected writes are ignored.
///
/// ```compile_fail
/// let io = relocade_machine::IoBus::default();
/// std::thread::spawn(move || io.read_port(0x3d4, 1));
/// ```
#[derive(Clone, Default)]
pub struct IoBus {
    connections: Rc<[DeviceConnection]>,
}

impl IoBus {
    pub fn is_empty(&self) -> bool {
        self.connections.is_empty()
    }

    /// Acknowledges the first ready request in connection order. Later requests
    /// remain with their devices; if none is ready every device sees the waker.
    pub fn poll_interrupt(&self, cx: &mut Context<'_>) -> Poll<u8> {
        for connection in &*self.connections {
            if let Poll::Ready(vector) = connection.device.borrow_mut().poll_interrupt(cx) {
                return Poll::Ready(vector);
            }
        }
        Poll::Pending
    }

    /// Connects each device's memory and port ranges together.
    ///
    /// # Panics
    /// Panics if a range is empty or two ranges on the same bus overlap.
    pub fn new(connections: impl IntoIterator<Item = DeviceConnection>) -> Self {
        let connections: Vec<_> = connections.into_iter().collect();
        validate_ranges(connections.iter().flat_map(|device| &device.mmio));
        validate_ranges(connections.iter().flat_map(|device| &device.ports));
        Self {
            connections: connections.into(),
        }
    }

    /// Reads one through eight bytes, splitting only at device boundaries.
    pub fn read_mmio(&self, address: u32, bytes: u8) -> u64 {
        check_mmio_range(address, bytes);
        let mut result = [0xff; 8];
        let mut done = 0;
        while done < bytes {
            let current = address + u32::from(done);
            let (device, count) = self.mmio_span(current, bytes - done);
            if let Some(device) = device {
                let value = device.borrow_mut().read_mmio(current, count).to_le_bytes();
                result[usize::from(done)..usize::from(done + count)]
                    .copy_from_slice(&value[..usize::from(count)]);
            }
            done += count;
        }
        // Unused carrier bytes do not form part of the transfer.
        result[usize::from(bytes)..].fill(0);
        u64::from_le_bytes(result)
    }

    /// Writes the low `bytes` bytes, splitting only at device boundaries.
    pub fn write_mmio(&self, address: u32, bytes: u8, value: u64) {
        check_mmio_range(address, bytes);
        let value = value.to_le_bytes();
        let mut done = 0;
        while done < bytes {
            let current = address + u32::from(done);
            let (device, count) = self.mmio_span(current, bytes - done);
            if let Some(device) = device {
                let mut part = [0; 8];
                part[..usize::from(count)]
                    .copy_from_slice(&value[usize::from(done)..usize::from(done + count)]);
                device
                    .borrow_mut()
                    .write_mmio(current, count, u64::from_le_bytes(part));
            }
            done += count;
        }
    }

    /// Routes one architectural transfer by its starting port. The device owns
    /// the meaning of a wide transfer, including paired index/data registers.
    pub fn read_port(&self, port: u16, bytes: u8) -> u32 {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        let value = self.port_device(port).map_or(u32::MAX, |device| {
            device.borrow_mut().read_port(port, bytes)
        });
        value & (u32::MAX >> ((4 - bytes) * 8))
    }

    pub fn write_port(&self, port: u16, bytes: u8, value: u32) {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        if let Some(device) = self.port_device(port) {
            device
                .borrow_mut()
                .write_port(port, bytes, value & (u32::MAX >> ((4 - bytes) * 8)));
        }
    }

    fn port_device(&self, port: u16) -> Option<&RefCell<dyn Device>> {
        self.connections
            .iter()
            .find(|connection| connection.ports.iter().any(|range| range.contains(&port)))
            .map(|connection| connection.device.as_ref())
    }

    fn mmio_span(&self, address: u32, bytes: u8) -> (Option<&RefCell<dyn Device>>, u8) {
        let mut count = u64::from(bytes);
        for connection in &*self.connections {
            for range in &connection.mmio {
                if range.contains(&address) {
                    count = count.min(u64::from(*range.end()) - u64::from(address) + 1);
                    return (Some(connection.device.as_ref()), count as u8);
                }
                if *range.start() > address {
                    count = count.min(u64::from(*range.start()) - u64::from(address));
                }
            }
        }
        (None, count as u8)
    }
}

fn validate_ranges<'a, T: Ord + Copy + 'a>(ranges: impl Iterator<Item = &'a RangeInclusive<T>>) {
    let mut ranges: Vec<_> = ranges.collect();
    assert!(
        ranges.iter().all(|range| !range.is_empty()),
        "empty device range"
    );
    ranges.sort_unstable_by_key(|range| *range.start());
    assert!(
        ranges
            .windows(2)
            .all(|pair| pair[0].end() < pair[1].start()),
        "overlapping device ranges"
    );
}

fn check_mmio_range(address: u32, bytes: u8) {
    assert!((1..=8).contains(&bytes), "MMIO transfer width");
    assert!(
        u64::from(address) + u64::from(bytes) <= 1 << 32,
        "MMIO address range"
    );
}
