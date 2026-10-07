//! Live physical routing metadata. No device access occurs during a lookup.

use wasm86_compiler::{BlockBuilder, BuildError, Mem, MemoryImport, Program, Val, I1, I32};

use super::super::{
    physical_map::{MMIO, PAGE_MASK, PAGE_SHIFT, RAM, ROM},
    DirectRange, Intent, PhysicalMemoryMap,
};

#[derive(Clone, Copy)]
pub(super) struct PhysicalTable {
    entries: Mem,
}

pub(super) struct Entry {
    pub(super) kind: Val<I32>,
    backing: Val<I32>,
}

impl Entry {
    pub(super) fn readable(&self) -> Val<I1> {
        self.kind.eq(RAM).or(self.kind.eq(ROM))
    }

    pub(super) fn backing_address(&self, address: &Val<I32>) -> Val<I32> {
        self.backing.add(address.and(PAGE_MASK))
    }
}

impl PhysicalTable {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            entries: program.import_memory(MemoryImport {
                module: "wasm86".into(),
                name: "physicalMap".into(),
                minimum: 1,
                maximum: None,
                shared: false,
            }),
        }
    }

    /// Callers have checked the complete segment span before physical lookup.
    pub(super) fn lookup(
        self,
        body: &mut BlockBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Entry, BuildError> {
        let offset = address.unsigned().shr(PAGE_SHIFT).shl(3);
        let kind = body.load_at::<I32>(self.entries, &offset, 0)?;
        let backing = body.load_at::<I32>(self.entries, &offset, 4)?;
        Ok(Entry { kind, backing })
    }

    /// One-page windows need no second lookup and never retain a device mapping.
    pub(super) fn direct_range(
        self,
        body: &mut BlockBuilder<'_>,
        address: &Val<I32>,
        bytes: u32,
        intent: Intent,
    ) -> Result<DirectRange, BuildError> {
        assert!((1..=PhysicalMemoryMap::PAGE_BYTES).contains(&bytes));
        let entry = self.lookup(body, address)?;
        let allowed = match intent {
            Intent::Read | Intent::Fetch => entry.readable(),
            Intent::Write => entry.kind.eq(RAM),
        };
        Ok(DirectRange {
            unavailable: allowed.eq(false).or(address
                .and(PAGE_MASK)
                .unsigned()
                .ge(PhysicalMemoryMap::PAGE_BYTES - bytes + 1)),
            physical: entry.backing_address(address),
        })
    }

    /// MMIO requests retain their span across adjacent MMIO pages. Ordinary
    /// backing uses single bytes in the slow loop. Every iteration observes
    /// routing after any preceding callback.
    pub(super) fn transfer_bytes(
        self,
        body: &mut BlockBuilder<'_>,
        entry: &Entry,
        address: &Val<I32>,
        remaining: &Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let in_page = Val::<I32>::from(PhysicalMemoryMap::PAGE_BYTES).sub(address.and(PAGE_MASK));
        let count = body.if_value::<I32>(
            in_page.unsigned().lt(remaining),
            |mut crossing| {
                let next_address = address.add(&in_page);
                let bytes = crossing.if_value::<I32>(
                    entry.kind.eq(MMIO),
                    |mut mmio| {
                        let next = self.lookup(&mut mmio, &next_address)?;
                        mmio.yield_(next.kind.eq(MMIO).select(remaining, &in_page))
                    },
                    |other| other.yield_(&in_page),
                )?;
                crossing.yield_(bytes)
            },
            |within| within.yield_(remaining),
        )?;
        Ok(entry.kind.eq(MMIO).select(count, 1))
    }
}
