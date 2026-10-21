use relocade_wasm_runtime::WasmSharedMemory;

/// Raw storage for an address space, independent of any CPU. Transfers are in
/// bounds. Shared storage does not serialize readers, writers or guest execution.
pub trait MemoryStorage {
    fn read_backing(&self, offset: u32, bytes: &mut [u8]);
    fn write_backing(&mut self, offset: u32, bytes: &[u8]);
}

impl<S: WasmSharedMemory> MemoryStorage for S {
    fn read_backing(&self, offset: u32, bytes: &mut [u8]) {
        self.read(offset, bytes);
    }

    fn write_backing(&mut self, offset: u32, bytes: &[u8]) {
        self.write(offset, bytes);
    }
}
