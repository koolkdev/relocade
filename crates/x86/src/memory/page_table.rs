//! Page translation and permission facts for spans touching at most two pages.

mod cache;

pub(crate) use cache::{PageCache, PageCacheInputs};

use wasm86_compiler::{BuildError, FunctionBuilder, Mem, MemoryImport, Program, Val, I1, I32};

const PAGE_SHIFT: u32 = 12;
pub(super) const PAGE_BYTES: u32 = 1 << PAGE_SHIFT;
const PAGE_OFFSET: u32 = PAGE_BYTES - 1;
pub(super) const FRAME_MASK: u32 = !PAGE_OFFSET;
pub(super) const PRESENT: u32 = 1;
pub(super) const WRITABLE: u32 = 2;
// A resolved range carries its first frame and permissions, or the denied page.
// These private flags distinguish scattered backing and a denial on the second page.
pub(super) const SCATTERED: u32 = 4;
pub(super) const LATER_DENIAL: u32 = 8;

#[derive(Clone, Copy)]
pub(super) struct PageTable {
    entries: Mem,
}

impl PageTable {
    pub(super) fn declare(program: &mut Program) -> Self {
        Self {
            entries: program.import_memory(MemoryImport {
                module: "wasm86".into(),
                name: "machine".into(),
                minimum: 64,
                maximum: None,
            }),
        }
    }

    pub(super) fn entry(
        self,
        body: &mut FunctionBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let index = address.unsigned().shr(PAGE_SHIFT).shl(2);
        body.load_at::<I32>(self.entries, index, 0)
    }

    /// Looks up the second page of a crossing span, reusing the first entry.
    /// The range must touch at most two pages.
    pub(super) fn lookup_span(
        self,
        body: &mut FunctionBuilder<'_>,
        start: &Val<I32>,
        last_byte_offset: impl Into<Val<I32>>,
        first_entry: &Val<I32>,
    ) -> Result<TwoPageSpan, BuildError> {
        let last_address = start.add(last_byte_offset);
        // Every page-table slot has backing, including a wrapped last index.
        // Looking up either entry is safe before classifying the range.
        let second_entry = self.entry(body, &last_address)?;
        Ok(TwoPageSpan {
            last_address,
            first_entry: first_entry.clone(),
            second_entry,
        })
    }

    // A separate generated function keeps the last-address calculation in the
    // cross-page path even when scattered accesses also use that address later.
    pub(super) fn define_range_resolver(
        self,
        mut body: FunctionBuilder<'_>,
    ) -> Result<(), BuildError> {
        let start = body.parameter::<I32>(0)?;
        let last_byte_offset = body.parameter::<I32>(1)?;
        let first_entry = body.parameter::<I32>(2)?;
        let required_permissions = body.parameter::<I32>(3)?;
        let span = self.lookup_span(&mut body, &start, last_byte_offset, &first_entry)?;
        body.if_(span.access_denied(&required_permissions), |arm| {
            arm.return_(span.encode_denial(&required_permissions))
        })?;
        body.return_(
            first_entry.and(FRAME_MASK | PRESENT | WRITABLE).or(span
                .has_scattered_backing()
                .unsigned()
                .extend::<I32>()
                .shl(2)),
        )
    }
}

/// Page-table facts for a range crossing one page boundary. Shared by the
/// contiguous-access check and detailed resolution; only resolution encodes denials.
pub(super) struct TwoPageSpan {
    last_address: Val<I32>,
    first_entry: Val<I32>,
    second_entry: Val<I32>,
}

impl TwoPageSpan {
    /// Linear addresses wrap; each translated page supplies its own permissions.
    pub(super) fn access_denied(
        &self,
        required_permissions: impl Into<Val<I32>> + Copy,
    ) -> Val<I1> {
        self.first_entry
            .and(&self.second_entry)
            .and(required_permissions)
            .ne(required_permissions)
    }

    pub(super) fn has_scattered_backing(&self) -> Val<I1> {
        let first_frame = self.first_entry.and(FRAME_MASK);
        let next_frame = first_frame.add(PAGE_BYTES);
        self.second_entry
            .and(FRAME_MASK)
            .ne(&next_frame)
            .or(next_frame.unsigned().lt(&first_frame))
    }

    fn encode_denial(&self, required_permissions: &Val<I32>) -> Val<I32> {
        let second_denial = self
            .last_address
            .and(FRAME_MASK)
            .or(self.second_entry.and(PRESENT))
            .or(LATER_DENIAL);
        let first_denial = self.first_entry.and(FRAME_MASK | PRESENT | WRITABLE);
        self.first_entry
            .and(required_permissions)
            .ne(required_permissions)
            .select(first_denial, second_denial)
    }
}

pub(super) fn crosses_page(start: &Val<I32>, bytes: u32) -> Val<I1> {
    start.and(PAGE_OFFSET).unsigned().ge(PAGE_BYTES - bytes + 1)
}

pub(super) fn physical_address(entry: &Val<I32>, address: &Val<I32>) -> Val<I32> {
    entry.and(FRAME_MASK).or(address.and(PAGE_OFFSET))
}
