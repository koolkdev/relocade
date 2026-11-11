use super::MemoryValue;
use crate::SharedMemory;

/// Sequentially consistent loads from an allocated, in-bounds range.
/// Use atomic operations for synchronization with Wasm or another host thread;
/// ordinary transfers do not provide a notification or acknowledgement protocol.
///
/// Only `u8` is supported. Native shared transfers use byte atomics, so wider
/// atomics need a separate access boundary to prevent unsynchronized overlapping
/// Rust accesses of different widths.
pub trait AtomicLoad<V: MemoryValue> {
    fn atomic_load(&self, offset: u32) -> V;
}

/// Sequentially consistent stores into an allocated, in-bounds range.
/// Supported types follow the same restriction as [`AtomicLoad`].
pub trait AtomicStore<V: MemoryValue> {
    fn atomic_store(&self, offset: u32, value: V);
}

/// Sequentially consistent exchanges in an allocated, in-bounds range.
/// Supported types follow the same restriction as [`AtomicLoad`].
pub trait AtomicSwap<V: MemoryValue> {
    /// Replaces the value and returns its previous value in one atomic operation.
    fn atomic_swap(&self, offset: u32, value: V) -> V;
}

impl SharedMemory {
    /// Atomically loads a value without borrowing a runtime. See [`AtomicLoad`]
    /// for ordering and supported types.
    ///
    /// Wider types are rejected at compile time:
    /// ```compile_fail
    /// use relocade_wasm_runtime::SharedMemory;
    /// fn load_word(memory: &SharedMemory) -> u32 {
    ///     memory.atomic_load::<u32>(0)
    /// }
    /// ```
    #[inline]
    pub fn atomic_load<V: MemoryValue>(&self, offset: u32) -> V
    where
        Self: AtomicLoad<V>,
    {
        AtomicLoad::atomic_load(self, offset)
    }

    /// Atomically stores a value without borrowing a runtime. See [`AtomicStore`]
    /// for ordering and supported types.
    #[inline]
    pub fn atomic_store<V: MemoryValue>(&self, offset: u32, value: V)
    where
        Self: AtomicStore<V>,
    {
        AtomicStore::atomic_store(self, offset, value);
    }

    /// Atomically replaces a value and returns its previous value without
    /// borrowing a runtime. See [`AtomicSwap`] for ordering and supported types.
    #[inline]
    pub fn atomic_swap<V: MemoryValue>(&self, offset: u32, value: V) -> V
    where
        Self: AtomicSwap<V>,
    {
        AtomicSwap::atomic_swap(self, offset, value)
    }
}
