mod io;
mod read;
mod write;

use super::{
    range_fits,
    resolve::{linear_address, segment_remaining, Spans},
    Memory, MemoryError,
};
use crate::{IoBus, IoContext, MachineConfig};
use io::Routing;
use relocade_wasm_runtime::{MemoryAccess, WasmSharedMemory};
use relocade_x86::SegmentRegister;
use std::cell::RefCell;

/// Guest physical memory through the configured map, including A20 aliases.
/// RAM/ROM use backing storage. MMIO requires a local I/O bus. Holes and
/// unconnected device ranges return FF; writes to them and ROM are ignored.
/// The view borrows one storage handle; other shared handles remain usable.
/// Accessing MMIO without a local bus is a programming error and panics.
pub struct MemoryView<'a> {
    memory: &'a Memory,
    config: &'a MachineConfig,
    storage: MemoryAccess<'a>,
    io: Option<Routing<'a>>,
}

impl<'a> MemoryView<'a> {
    /// Borrows backing storage matching the configured map and capacity.
    #[inline]
    pub(super) fn new(memory: &'a Memory) -> Self {
        Self {
            memory,
            config: &memory.config,
            storage: memory.backing.view(),
            io: None,
        }
    }

    /// Enables device accesses through the execution worker's bus. CPU contexts
    /// already supply it; independent RAM views need not retain any devices.
    pub fn with_io(mut self, io: &'a IoBus) -> Self {
        self.io = Some(Routing::Bus(io));
        self
    }

    /// Uses a device callback's existing runtime scope for mapped memory access.
    /// MMIO forwards through this context without borrowing the runtime again.
    pub fn with_io_context(mut self, context: IoContext<'a>) -> Self {
        self.io = Some(Routing::Context {
            bus: context.io(),
            context: RefCell::new(context),
        });
        self
    }

    /// Configured backing capacity, excluding Wasm-page rounding.
    pub fn backing_size(&self) -> u64 {
        self.config.memory_size()
    }
}

fn segment_address(segment: &SegmentRegister, offset: u32, len: usize) -> Result<u32, MemoryError> {
    if len as u64 > segment_remaining(segment, offset)? {
        return Err(MemoryError::SegmentRange);
    }
    linear_address(segment, offset)
}

#[cfg(test)]
mod tests;
