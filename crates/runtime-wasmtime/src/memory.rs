use wasm86_machine::Environment;
use wasm86_runtime::backend::{GuestMemory, Imports};
use wasmtime::{AsContextMut, Memory};

struct MemoryAccess<'a> {
    backing: &'a mut [u8],
    map_update: Option<Vec<u8>>,
}

impl GuestMemory for MemoryAccess<'_> {
    fn byte_len(&self) -> u64 {
        self.backing.len() as u64
    }
    fn read(&self, offset: u32, bytes: &mut [u8]) {
        bytes.copy_from_slice(&self.backing[offset as usize..][..bytes.len()]);
    }
    fn write(&mut self, offset: u32, bytes: &[u8]) {
        self.backing[offset as usize..][..bytes.len()].copy_from_slice(bytes);
    }
    fn publish_map(&mut self, bytes: &[u8]) {
        self.map_update = Some(bytes.to_vec());
    }
}

/// The Wasmtime store lends guest bytes and import state together. Publish a new
/// table after that borrow ends, before either a host access or callback returns.
/// Publication also occurs when the access closure returns a Result::Err value.
pub(crate) fn access<D: Environment, R>(
    mut store: impl AsContextMut<Data = Imports<D>>,
    guest: Memory,
    mapping: Memory,
    access: impl FnOnce(&mut Imports<D>, &mut dyn GuestMemory) -> R,
) -> R {
    let mut store = store.as_context_mut();
    let (backing, imports) = guest.data_and_store_mut(&mut store);
    let mut memory = MemoryAccess {
        backing,
        map_update: None,
    };
    let result = access(imports, &mut memory);
    if let Some(bytes) = memory.map_update {
        mapping
            .write(&mut store, 0, &bytes)
            .expect("mapping table has fixed valid backing");
    }
    result
}
