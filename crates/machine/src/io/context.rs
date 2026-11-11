use super::State;
use relocade_wasm_runtime::TableContext;
use std::rc::Weak;

/// Scoped device access during a guest callback or synchronous host operation.
/// Every transfer receives this as its first argument. Native callbacks borrow
/// Wasmtime's Caller; browser callbacks need no runtime-state borrow.
pub struct IoContext<'a> {
    pub(crate) runtime: TableContext<'a, ()>,
    pub(crate) bus: &'a Weak<State>,
}

impl<'a> IoContext<'a> {
    pub(crate) fn new(runtime: TableContext<'a, ()>, bus: &'a Weak<State>) -> Self {
        Self { runtime, bus }
    }

    /// Borrows this context for another synchronous operation.
    pub fn reborrow(&mut self) -> IoContext<'_> {
        IoContext::new(self.runtime.reborrow(), self.bus)
    }

    /// Reads through this bus without reacquiring its runtime.
    pub fn read_port(&mut self, port: u16, bytes: u8) -> u32 {
        let bus = self.bus.upgrade().expect("live I/O bus");
        bus.read_port(self.reborrow(), port, bytes)
    }

    /// Writes through this bus without reacquiring its runtime.
    pub fn write_port(&mut self, port: u16, bytes: u8, value: u32) {
        let bus = self.bus.upgrade().expect("live I/O bus");
        bus.write_port(self.reborrow(), port, bytes, value)
    }

    /// Reads one through eight bytes through this bus's MMIO routing.
    pub fn read_mmio(&mut self, address: u32, bytes: u8) -> u64 {
        let bus = self.bus.upgrade().expect("live I/O bus");
        bus.read_mmio(self.reborrow(), address, bytes)
    }

    /// Writes one through eight bytes through this bus's MMIO routing.
    pub fn write_mmio(&mut self, address: u32, bytes: u8, value: u64) {
        let bus = self.bus.upgrade().expect("live I/O bus");
        bus.write_mmio(self.reborrow(), address, bytes, value)
    }
}
