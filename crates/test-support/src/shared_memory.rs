//! Atomic test-host access to shared Wasm memory.

use std::sync::atomic::{AtomicU8, Ordering};
use wasmtime::{MemoryType, SharedMemory};

/// Shared bytes for test setup and inspection. Tests coordinate whole snapshots
/// themselves; a multi-byte copy is not atomic.
#[derive(Clone)]
pub struct SharedBytes {
    memory: SharedMemory,
}

impl SharedBytes {
    pub fn new(minimum: u32, maximum: u32) -> Self {
        let memory =
            SharedMemory::new(crate::engine(), MemoryType::shared(minimum, maximum)).unwrap();
        Self { memory }
    }

    pub fn memory(&self) -> &SharedMemory {
        &self.memory
    }

    pub fn read(&self, offset: u32, length: usize) -> Vec<u8> {
        self.range(offset, length)
            .iter()
            .map(|byte| byte.load(Ordering::Relaxed))
            .collect()
    }

    pub fn write(&self, offset: u32, bytes: &[u8]) {
        for (cell, byte) in self.range(offset, bytes.len()).iter().zip(bytes) {
            cell.store(*byte, Ordering::Relaxed);
        }
    }

    fn range(&self, offset: u32, len: usize) -> &[AtomicU8] {
        let start = offset as usize;
        let end = start.checked_add(len).expect("test memory range overflow");
        let cells = &self.memory.data()[start..end];
        // SAFETY: Wasmtime keeps shared data allocated at a stable address while
        // this handle lives, including across growth. AtomicU8 has byte size and
        // alignment. The range is checked, and all Rust accesses use byte atomics.
        unsafe { std::slice::from_raw_parts(cells.as_ptr().cast::<AtomicU8>(), cells.len()) }
    }
}
