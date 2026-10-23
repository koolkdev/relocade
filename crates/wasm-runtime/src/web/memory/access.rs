use super::super::{Backend, Context, Memory, Store};
use crate::{MemoryValue, WasmMemoryAccess};

impl<T> WasmMemoryAccess for Backend<T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        self.store.borrow().read_memory(memory, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        self.store.borrow_mut().write_memory(memory, offset, bytes);
    }

    fn load_memory<V: MemoryValue>(&self, memory: &Memory, offset: u32) -> V {
        self.store.borrow().load_memory(memory, offset)
    }

    fn store_memory<V: MemoryValue>(&mut self, memory: &Memory, offset: u32, value: V) {
        self.store.borrow_mut().store_memory(memory, offset, value);
    }
}

impl<T> WasmMemoryAccess for Store<T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        self.memory(memory).transfers.read(offset, bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        self.memory(memory).transfers.write(offset, bytes);
    }

    fn load_memory<V: MemoryValue>(&self, memory: &Memory, offset: u32) -> V {
        self.memory(memory).transfers.load(offset)
    }

    fn store_memory<V: MemoryValue>(&mut self, memory: &Memory, offset: u32, value: V) {
        self.memory(memory).transfers.store(offset, value);
    }
}

impl<T> WasmMemoryAccess for Context<'_, T> {
    type Memory = Memory;

    fn read_memory(&self, memory: &Memory, offset: u32, bytes: &mut [u8]) {
        self.0.read_memory(memory, offset, bytes);
    }

    fn write_memory(&mut self, memory: &Memory, offset: u32, bytes: &[u8]) {
        self.0.write_memory(memory, offset, bytes);
    }

    fn load_memory<V: MemoryValue>(&self, memory: &Memory, offset: u32) -> V {
        self.0.load_memory(memory, offset)
    }

    fn store_memory<V: MemoryValue>(&mut self, memory: &Memory, offset: u32, value: V) {
        self.0.store_memory(memory, offset, value);
    }
}
