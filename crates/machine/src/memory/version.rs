//! Mapping revisions for invalidating generated code. The embedding coordinates
//! edits with address-space users; revision storage provides no mutual exclusion.

use relocade_wasm_runtime::MemoryBytes;

pub(crate) const BYTE_LEN: u64 = 8;

pub(crate) fn generation(revision: &MemoryBytes<'_>) -> u64 {
    let mut bytes = [0; 8];
    revision.read(0, &mut bytes);
    u64::from_le_bytes(bytes)
}

pub(crate) fn changed(revision: &mut MemoryBytes<'_>) {
    let next = next_generation(generation(revision));
    revision.write(0, &next.to_le_bytes());
}

pub(crate) fn next_generation(current: u64) -> u64 {
    current
        .checked_add(1)
        .expect("mapping generation exhausted")
}
