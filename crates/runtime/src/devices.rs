use crate::{machine::State, memory::MemoryBus, DeviceMemory};
use wasmtime::{Caller, Linker};

/// Synchronous device bus for one guest CPU. Methods complete each transfer once;
/// asynchronous work must be handled by the embedding between execution entries.
/// Port widths are 1, 2 or 4 bytes; MMIO widths are 1 through 8, in little endian.
/// Default methods model an open bus: all-one reads and ignored writes.
pub trait Devices: Send {
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

pub(crate) fn register<D: Devices + 'static>(
    linker: &mut Linker<State<D>>,
    memory: MemoryBus,
) -> wasmtime::Result<()> {
    linker.func_wrap(
        "wasm86",
        "readPort",
        move |mut caller: Caller<'_, State<D>>, port: u32, bytes: u32| {
            memory.access(&mut caller, |devices, memory| {
                devices.read_port(memory, port as u16, bytes as u8)
            })
        },
    )?;
    linker.func_wrap(
        "wasm86",
        "writePort",
        move |mut caller: Caller<'_, State<D>>, port: u32, bytes: u32, value: u32| {
            memory.access(&mut caller, |devices, memory| {
                devices.write_port(memory, port as u16, bytes as u8, value)
            })
        },
    )?;
    linker.func_wrap(
        "wasm86",
        "readMmio",
        move |mut caller: Caller<'_, State<D>>, address: u32, bytes: u32| {
            memory.access(&mut caller, |devices, memory| {
                devices.read_mmio(memory, address, bytes as u8)
            })
        },
    )?;
    linker.func_wrap(
        "wasm86",
        "writeMmio",
        move |mut caller: Caller<'_, State<D>>, address: u32, bytes: u32, value: u64| {
            memory.access(&mut caller, |devices, memory| {
                devices.write_mmio(memory, address, bytes as u8, value)
            })
        },
    )?;
    Ok(())
}
