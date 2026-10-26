use crate::{Error, Load, MemoryAccess, MemoryValue, Result, Store, WasmSharedMemory};
use relocade_compiler::abi;
use std::sync::atomic::{AtomicU8, Ordering};
use wasmtime::{AsContext, AsContextMut, Extern, MemoryType};

/// A private store memory or an independently owned shared memory.
#[derive(Clone)]
pub enum Memory {
    Private(wasmtime::Memory),
    Shared(SharedMemory),
}

/// Shared linear memory that can be read without borrowing the Wasmtime store.
#[derive(Clone)]
pub struct SharedMemory(wasmtime::SharedMemory);

pub(crate) enum MemoryRegion<'a> {
    Private(&'a mut [u8]),
    Shared(&'a SharedMemory),
}

impl MemoryRegion<'_> {
    pub(crate) fn reborrow(&mut self) -> MemoryRegion<'_> {
        match self {
            Self::Private(bytes) => MemoryRegion::Private(bytes),
            Self::Shared(memory) => MemoryRegion::Shared(memory),
        }
    }

    pub(crate) fn read(&self, offset: u32, bytes: &mut [u8]) {
        match self {
            Self::Private(data) => bytes.copy_from_slice(&data[offset as usize..][..bytes.len()]),
            Self::Shared(memory) => memory.read(offset, bytes),
        }
    }

    pub(crate) fn write(&mut self, offset: u32, bytes: &[u8]) {
        match self {
            Self::Private(data) => data[offset as usize..][..bytes.len()].copy_from_slice(bytes),
            Self::Shared(memory) => memory.write(offset, bytes),
        }
    }
}

impl Memory {
    pub(super) fn new(mut store: impl AsContextMut, declaration: abi::Memory) -> Result<Self> {
        if declaration.shared {
            let maximum = declaration.maximum.ok_or_else(|| {
                Error::new("create Wasm memory", "shared memory requires a maximum")
            })?;
            wasmtime::SharedMemory::new(
                store.as_context().engine(),
                MemoryType::shared(declaration.minimum, maximum),
            )
            .map(|memory| Self::Shared(SharedMemory(memory)))
            .map_err(|error| Error::new("create Wasm memory", error))
        } else {
            wasmtime::Memory::new(
                &mut store,
                MemoryType::new(declaration.minimum, declaration.maximum),
            )
            .map(Self::Private)
            .map_err(|error| Error::new("create Wasm memory", error))
        }
    }

    pub(super) fn access<'a, T>(
        &'a self,
        store: wasmtime::StoreContextMut<'a, T>,
    ) -> MemoryAccess<'a> {
        match self {
            Self::Private(memory) => {
                MemoryAccess::new(MemoryRegion::Private(memory.data_mut(store)))
            }
            Self::Shared(memory) => memory.view(),
        }
    }

    pub(super) fn ensure_size(&self, mut store: impl AsContextMut, byte_len: u64) -> Result<()> {
        let pages = byte_len.div_ceil(65536);
        let current = self.size(&store);
        if pages > current {
            let growth = match self {
                Self::Private(memory) => memory.grow(&mut store, pages - current),
                Self::Shared(memory) => memory.0.grow(pages - current),
            };
            // Another owner may have grown shared memory since the size check.
            if let Err(error) = growth {
                if self.size(&store) < pages {
                    return Err(Error::new("grow Wasm memory", error));
                }
            }
        }
        Ok(())
    }

    fn size(&self, store: impl AsContext) -> u64 {
        match self {
            Self::Private(memory) => memory.size(store),
            Self::Shared(memory) => memory.0.size(),
        }
    }

    pub(super) fn as_extern(&self) -> Extern {
        match self {
            Self::Private(memory) => (*memory).into(),
            Self::Shared(memory) => memory.0.clone().into(),
        }
    }
}

impl WasmSharedMemory for SharedMemory {
    fn view(&self) -> MemoryAccess<'_> {
        MemoryAccess::new(MemoryRegion::Shared(self))
    }

    fn byte_len(&self) -> u64 {
        self.0.size() * 65536
    }

    fn write(&self, offset: u32, bytes: &[u8]) {
        for (cell, byte) in self.range(offset, bytes.len()).iter().zip(bytes) {
            cell.store(*byte, Ordering::Relaxed);
        }
    }

    fn read(&self, offset: u32, bytes: &mut [u8]) {
        for (cell, byte) in self.range(offset, bytes.len()).iter().zip(bytes) {
            *byte = cell.load(Ordering::Relaxed);
        }
    }
}

impl SharedMemory {
    #[allow(unsafe_code)]
    fn range(&self, offset: u32, len: usize) -> &[AtomicU8] {
        let data = self.0.data();
        let start = offset as usize;
        let end = start.checked_add(len).expect("Wasm memory range overflow");
        let cells = &data[start..end];
        // SAFETY: Wasmtime owns this allocation and keeps it stable, including
        // across growth, while this SharedMemory exists. AtomicU8 has byte size
        // and alignment. The checked range is initialized and remains allocated.
        // All Rust accesses to shared contents use byte atomics; guest Wasm
        // accesses obey Wasmtime's shared-memory contract.
        unsafe { std::slice::from_raw_parts(cells.as_ptr().cast::<AtomicU8>(), cells.len()) }
    }
}

impl<V: MemoryValue> Load<V> for MemoryRegion<'_> {
    #[inline]
    fn load(&self, offset: u32) -> V {
        let mut bytes = V::Bytes::default();
        self.read(offset, bytes.as_mut());
        V::from_le_bytes(bytes)
    }
}

impl<V: MemoryValue> Store<V> for MemoryRegion<'_> {
    #[inline]
    fn store(&mut self, offset: u32, value: V) {
        self.write(offset, value.to_le_bytes().as_ref());
    }
}
