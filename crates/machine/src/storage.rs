use relocade_wasm_runtime::{MemoryValue, WasmSharedMemory};

/// A byte offset relative to the start of the CPU state.
#[derive(Clone, Copy)]
pub struct CpuOffset(pub u32);

/// One scalar transfer. The address type identifies the storage and the value
/// type fixes its width, including when called through an erased storage owner.
pub trait ScalarAccess<A, V: MemoryValue> {
    fn load(&self, address: A) -> V;
    fn store(&mut self, address: A, value: V);
}

/// Live storage with little-endian scalar and byte transfers. Ranges must be in
/// bounds; offsets may be unaligned. Shared storage does not serialize accesses
/// or provide snapshots. Plain `u32` addresses identify guest backing offsets.
///
/// The supported scalar types remain separate vtable entries at the erased
/// boundary, while implementations can forward them with one generic pair.
pub trait MemoryStorage<A = u32>:
    ScalarAccess<A, u8> + ScalarAccess<A, u16> + ScalarAccess<A, u32> + ScalarAccess<A, u64>
{
    fn read(&self, address: A, bytes: &mut [u8]);
    fn write(&mut self, address: A, bytes: &[u8]);
}

impl<S: WasmSharedMemory, V: MemoryValue> ScalarAccess<u32, V> for S {
    fn load(&self, offset: u32) -> V {
        WasmSharedMemory::load(self, offset)
    }

    fn store(&mut self, offset: u32, value: V) {
        WasmSharedMemory::store(self, offset, value);
    }
}

impl<S: WasmSharedMemory> MemoryStorage for S {
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        WasmSharedMemory::read(self, offset, bytes);
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) {
        WasmSharedMemory::write(self, offset, bytes);
    }
}

/// Live CPU state and guest backing supplied by a runtime. CPU writes take
/// effect immediately; implementations must not cache a CPU snapshot.
pub trait CpuStorage: MemoryStorage<CpuOffset> + MemoryStorage {}

impl<S: MemoryStorage<CpuOffset> + MemoryStorage + ?Sized> CpuStorage for S {}
