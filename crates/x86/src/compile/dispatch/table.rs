//! Address-table representation and its generated lookup stay together.

use super::Dispatch;
use relocade_compiler::{BlockBuilder, BuildError, Val, I32};
use std::collections::BTreeMap;

const HASH_MULTIPLIER: u32 = 0x9e37_79b1;
/// Linear execution addresses mapped to ordinary `() -> i64` function-table slots.
/// Slot zero is reserved; slot one holds the interpreter fallback.
/// The embedding installs functions before publishing their addresses. Keys are
/// CS.base + EIP before paging/A20 translation, scoped to one execution target
/// and address space. This table neither classifies entries nor validates code.
#[derive(Clone, Debug, Default)]
pub struct EntryTable(BTreeMap<u32, u32>);

impl EntryTable {
    pub const INTERPRETER_SLOT: u32 = 1;
    pub const FIRST_DYNAMIC_SLOT: u32 = 2;

    pub fn get(&self, address: u32) -> Option<u32> {
        self.0.get(&address).copied()
    }

    /// Publishes a callable slot. Zero is reserved for empty rows.
    pub fn insert(&mut self, address: u32, slot: u32) -> Option<u32> {
        assert_ne!(slot, 0, "slot zero is reserved for empty rows");
        self.0.insert(address, slot)
    }

    pub fn remove(&mut self, address: u32) -> Option<u32> {
        self.0.remove(&address)
    }

    /// Serializes a mask followed by address/slot rows for open-addressed lookup.
    /// At most half the rows are occupied, including when the table is empty.
    /// Write the complete result at offset zero in `relocade.executionEntries`.
    pub fn to_bytes(&self) -> Vec<u8> {
        let capacity = (self.0.len() * 2).max(2).next_power_of_two();
        let mask = u32::try_from(capacity - 1).expect("entry table fits Wasm memory");
        let mut rows = vec![(0u32, 0u32); capacity];
        for (&address, &slot) in &self.0 {
            let product = address.wrapping_mul(HASH_MULTIPLIER);
            let mut bucket = (product ^ (product >> 16)) & mask;
            while rows[bucket as usize].1 != 0 {
                bucket = (bucket + 1) & mask;
            }
            rows[bucket as usize] = (address, slot);
        }
        let mut bytes = Vec::with_capacity(4 + capacity * 8);
        bytes.extend_from_slice(&mask.to_le_bytes());
        for (address, slot) in rows {
            bytes.extend_from_slice(&address.to_le_bytes());
            bytes.extend_from_slice(&slot.to_le_bytes());
        }
        bytes
    }
}

impl Dispatch {
    pub(super) fn lookup(
        &self,
        mut body: BlockBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<(), BuildError> {
        let mask = body.load_at::<I32>(self.memory, 0, 0)?;
        let product = address.mul(HASH_MULTIPLIER);
        let bucket = product.xor(product.unsigned().shr(16)).and(&mask);
        body.loop_::<I32, ()>(bucket, |mut probe, labels, bucket| {
            let row = bucket.mul(8).add(4);
            let slot = probe.load_at::<I32>(self.memory, &row, 4)?;
            probe.if_(slot.eq(0), |miss| {
                self.enter(miss, EntryTable::INTERPRETER_SLOT)
            })?;
            let candidate = probe.load_at::<I32>(self.memory, &row, 0)?;
            probe.if_(candidate.eq(address), |hit| self.enter(hit, slot))?;
            probe.branch(&labels.again, bucket.add(1).and(&mask))
        })?;
        body.trap()
    }
}
