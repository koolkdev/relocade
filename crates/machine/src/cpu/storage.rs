//! Raw storage access through a stopped CPU's backend.

use crate::{Cpu, CpuStorage, MemoryStorage, RuntimeBackend};

impl<B: RuntimeBackend> MemoryStorage for Cpu<B> {
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        self.bindings.read_backing(&self.backend, offset, bytes);
    }
    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.bindings
            .write_backing(&mut self.backend, offset, bytes);
    }
}

impl<B: RuntimeBackend> CpuStorage for Cpu<B> {
    fn read_cpu(&self, offset: u32, bytes: &mut [u8]) {
        self.bindings.read_cpu(&self.backend, offset, bytes);
    }
    fn write_cpu(&mut self, offset: u32, bytes: &[u8]) {
        self.bindings.write_cpu(&mut self.backend, offset, bytes);
    }
}
