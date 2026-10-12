use crate::DeviceMemory;

/// Synchronous device bus for one guest CPU. Methods complete each transfer once;
/// asynchronous work must be handled by the embedding between execution entries.
/// Port widths are 1, 2 or 4 bytes; MMIO widths are 1 through 8, in little endian.
/// Default methods model an open bus: all-one reads and ignored writes.
/// Device state need not be `Send`; thread requirements belong to the embedding.
pub trait Devices {
    fn read_port(&mut self, _memory: &mut DeviceMemory<'_>, _port: u16, _bytes: u8) -> u32 {
        u32::MAX
    }
    fn write_port(&mut self, _memory: &mut DeviceMemory<'_>, _port: u16, _bytes: u8, _value: u32) {}
    fn read_mmio(&mut self, _memory: &mut DeviceMemory<'_>, _address: u32, _bytes: u8) -> u64 {
        u64::MAX
    }
    fn write_mmio(
        &mut self,
        _memory: &mut DeviceMemory<'_>,
        _address: u32,
        _bytes: u8,
        _value: u64,
    ) {
    }
}

impl Devices for () {}
