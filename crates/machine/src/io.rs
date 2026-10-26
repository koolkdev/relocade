//! Fixed device connections shared by guest execution and host services.

use std::{ops::RangeInclusive, sync::Arc};

/// A synchronous device on the machine's memory and port buses.
/// Addresses are physical bus addresses, after A20 translation for MMIO.
/// Values are little endian. MMIO transfers contain one through eight bytes;
/// port transfers retain their architectural width of one, two or four bytes.
///
/// Implementations own their mutable state and synchronize access with display
/// readers or other host threads. Callbacks must not reenter guest execution.
pub trait Device: Send + Sync {
    fn read_mmio(&self, _address: u32, _bytes: u8) -> u64 {
        u64::MAX
    }

    fn write_mmio(&self, _address: u32, _bytes: u8, _value: u64) {}

    fn read_port(&self, _port: u16, _bytes: u8) -> u32 {
        u32::MAX
    }

    fn write_port(&self, _port: u16, _bytes: u8, _value: u32) {}
}

/// The addresses decoded by one device. Memory ranges must also be marked
/// [`relocade_x86::PhysicalMapping::Mmio`] in the machine's physical map.
pub struct DeviceConnection {
    pub device: Arc<dyn Device>,
    pub mmio: Vec<RangeInclusive<u32>>,
    pub ports: Vec<RangeInclusive<u16>>,
}

/// Immutable device routing. Clones retain the same connections and device state.
/// Unconnected reads return FF bytes and unconnected writes are ignored.
#[derive(Clone, Default)]
pub struct IoBus {
    connections: Arc<[DeviceConnection]>,
}

impl IoBus {
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
                let value = device.read_mmio(current, count).to_le_bytes();
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
                device.write_mmio(current, count, u64::from_le_bytes(part));
            }
            done += count;
        }
    }

    /// Routes one architectural transfer by its starting port. The device owns
    /// the meaning of a wide transfer, including paired index/data registers.
    pub fn read_port(&self, port: u16, bytes: u8) -> u32 {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        let value = self
            .port_device(port)
            .map_or(u32::MAX, |device| device.read_port(port, bytes));
        value & (u32::MAX >> ((4 - bytes) * 8))
    }

    pub fn write_port(&self, port: u16, bytes: u8, value: u32) {
        assert!(matches!(bytes, 1 | 2 | 4), "port transfer width");
        if let Some(device) = self.port_device(port) {
            device.write_port(port, bytes, value & (u32::MAX >> ((4 - bytes) * 8)));
        }
    }

    fn port_device(&self, port: u16) -> Option<&dyn Device> {
        self.connections
            .iter()
            .find(|connection| connection.ports.iter().any(|range| range.contains(&port)))
            .map(|connection| connection.device.as_ref())
    }

    fn mmio_span(&self, address: u32, bytes: u8) -> (Option<&dyn Device>, u8) {
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
