//! Device routing and synchronous access on one execution worker.

mod device;
pub use device::{Device, DeviceConnection};

use crate::runtime::Runtime;
use std::{
    cell::{OnceCell, RefCell},
    ops::RangeInclusive,
    rc::{Rc, Weak},
    task::{Context, Poll},
};

/// Device connections on one execution worker. Clones share routing and the
/// retained runtime. One CPU may attach to this bus.
/// Unconnected reads return FF bytes and unconnected writes are ignored.
///
/// ```compile_fail
/// let io = relocade_machine::IoBus::default();
/// std::thread::spawn(move || io.read_port(0x3d4, 1));
/// ```
#[derive(Clone)]
pub struct IoBus {
    state: Rc<State>,
    runtime: Runtime,
}

pub(crate) struct State {
    connections: OnceCell<Box<[DeviceConnection]>>,
    identity: Weak<State>,
}

impl Default for IoBus {
    fn default() -> Self {
        Self {
            state: Rc::new_cyclic(|identity| State {
                connections: OnceCell::new(),
                identity: identity.clone(),
            }),
            runtime: Runtime::default(),
        }
    }
}

impl IoBus {
    /// Connects each device's memory and port ranges together.
    ///
    /// # Panics
    /// Panics if a range is empty or two ranges on the same bus overlap.
    pub fn new(connections: impl IntoIterator<Item = DeviceConnection>) -> Self {
        let io = Self::default();
        io.connect(connections);
        io
    }

    /// Initializes this bus's device connections once. The CPU keeps this same
    /// bus throughout its lifetime; devices may be connected after CPU creation.
    ///
    /// # Panics
    /// Panics if connections are already initialized, or a range is empty or overlaps.
    pub fn connect(&self, connections: impl IntoIterator<Item = DeviceConnection>) {
        assert!(
            self.state.connections.get().is_none(),
            "devices already connected"
        );
        let connections: Box<[_]> = connections.into_iter().collect();
        validate_ranges(connections.iter().flat_map(|device| &device.mmio));
        validate_ranges(connections.iter().flat_map(|device| &device.ports));
        self.state
            .connections
            .set(connections)
            .unwrap_or_else(|_| unreachable!());
    }

    pub fn is_empty(&self) -> bool {
        self.state.is_empty()
    }

    /// Acknowledges the first ready interrupt in connection order.
    pub fn poll_interrupt(&self, cx: &mut Context<'_>) -> Poll<u8> {
        self.state.poll_interrupt(cx)
    }

    /// Reads one through eight bytes, splitting only at device boundaries.
    pub fn read_mmio(&self, address: u32, bytes: u8) -> u64 {
        self.state.read_mmio(address, bytes)
    }

    /// Writes the low `bytes` bytes, splitting only at device boundaries.
    pub fn write_mmio(&self, address: u32, bytes: u8, value: u64) {
        self.state.write_mmio(address, bytes, value)
    }

    /// Reads 1, 2 or 4 bytes from the device at `port` in one call. Unused result
    /// bits are zero; the device owns the meaning of word and dword transfers.
    pub fn read_port(&self, port: u16, bytes: u8) -> u32 {
        self.state.read_port(port, bytes)
    }

    /// Writes the low 1, 2 or 4 bytes to the device at `port` in one call.
    pub fn write_port(&self, port: u16, bytes: u8, value: u32) {
        self.state.write_port(port, bytes, value)
    }

    pub(crate) fn downgrade(&self) -> Weak<State> {
        self.state.identity.clone()
    }

    pub(crate) fn runtime(&self) -> &Runtime {
        &self.runtime
    }
}

impl State {
    fn connections(&self) -> &[DeviceConnection] {
        self.connections.get().map_or(&[], Box::as_ref)
    }

    pub fn is_empty(&self) -> bool {
        self.connections().is_empty()
    }

    pub fn poll_interrupt(&self, cx: &mut Context<'_>) -> Poll<u8> {
        for connection in self.connections() {
            if let Poll::Ready(vector) = connection.device.borrow_mut().poll_interrupt(cx) {
                return Poll::Ready(vector);
            }
        }
        Poll::Pending
    }

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

    pub fn read_port(&self, port: u16, bytes: u8) -> u32 {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        self.read_port_unchecked(port, bytes) & (u32::MAX >> ((4 - bytes) * 8))
    }

    pub fn write_port(&self, port: u16, bytes: u8, value: u32) {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        self.write_port_unchecked(port, bytes, value & (u32::MAX >> ((4 - bytes) * 8)));
    }

    #[inline]
    pub(crate) fn read_port_unchecked(&self, port: u16, bytes: u8) -> u32 {
        debug_assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        self.port_device(port).map_or(u32::MAX, |device| {
            device.borrow_mut().read_port(port, bytes)
        })
    }

    #[inline]
    pub(crate) fn write_port_unchecked(&self, port: u16, bytes: u8, value: u32) {
        debug_assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        debug_assert_eq!(value & (u32::MAX >> ((4 - bytes) * 8)), value);
        if let Some(device) = self.port_device(port) {
            device.borrow_mut().write_port(port, bytes, value);
        }
    }

    fn port_device(&self, port: u16) -> Option<&RefCell<dyn Device>> {
        self.connections()
            .iter()
            .find(|connection| connection.ports.iter().any(|range| range.contains(&port)))
            .map(|connection| connection.device.as_ref())
    }

    fn mmio_span(&self, address: u32, bytes: u8) -> (Option<&RefCell<dyn Device>>, u8) {
        let mut count = u64::from(bytes);
        for connection in self.connections() {
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
