//! Page translation, permissions and physical contiguity for complete byte spans.

mod cache;

pub(crate) use cache::{PageCache, PageCacheInputs};

use wasm86_compiler::{BlockBuilder, BuildError, Mem, MemoryImport, Program, Val, I1, I32};

const PAGE_SHIFT: u32 = 12;
pub(super) const PAGE_BYTES: u32 = 1 << PAGE_SHIFT;
const PAGE_OFFSET: u32 = PAGE_BYTES - 1;
pub(super) const FRAME_MASK: u32 = !PAGE_OFFSET;
pub(super) const PRESENT: u32 = 1;
pub(super) const WRITABLE: u32 = 2;
// A resolved range carries its first frame and permissions, or the denied page.
// These private flags distinguish scattered backing and a denial on a later page.
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
                shared: false,
            }),
        }
    }

    pub(super) fn entry(
        self,
        body: &mut BlockBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let index = address.unsigned().shr(PAGE_SHIFT).shl(2);
        body.load_at::<I32>(self.entries, index, 0)
    }

    /// Resolves every touched page of a nonempty span. The result carries its
    /// first frame and permissions, plus scattered backing, or the first denial.
    pub(super) fn define_range_resolver(
        self,
        mut body: BlockBuilder<'_>,
    ) -> Result<(), BuildError> {
        let start = body.parameter::<I32>(0)?;
        let last_byte_offset = body.parameter::<I32>(1)?;
        let first_entry = body.parameter::<I32>(2)?;
        let required = body.parameter::<I32>(3)?;
        body.if_(first_entry.and(&required).ne(&required), |arm| {
            arm.return_(first_entry.and(FRAME_MASK | PRESENT | WRITABLE))
        })?;
        // Count page boundaries without overflowing start + last_byte_offset.
        // The linear page address wraps independently while walking this count.
        let boundaries = last_byte_offset.unsigned().shr(PAGE_SHIFT).add(
            start
                .and(PAGE_OFFSET)
                .add(last_byte_offset.and(PAGE_OFFSET))
                .unsigned()
                .shr(PAGE_SHIFT),
        );
        let scattered = body.loop_::<(I32, I32, I1, I32), I1>(
            (
                start.and(FRAME_MASK),
                first_entry.and(FRAME_MASK),
                false,
                boundaries,
            ),
            |mut page, labels, (address, frame, scattered, remaining)| {
                page.branch_if(remaining.eq(0), &labels.exit, &scattered)?;
                let next_address = address.add(PAGE_BYTES);
                let entry = self.entry(&mut page, &next_address)?;
                page.if_(entry.and(&required).ne(&required), |arm| {
                    arm.return_(next_address.or(entry.and(PRESENT)).or(LATER_DENIAL))
                })?;
                page.branch(
                    &labels.again,
                    (
                        next_address,
                        entry.and(FRAME_MASK),
                        scattered.or(scattered_backing(&frame, &entry)),
                        remaining.sub(1),
                    ),
                )
            },
        )?;
        body.return_(
            first_entry
                .and(FRAME_MASK | PRESENT | WRITABLE)
                .or(scattered.unsigned().extend::<I32>().shl(2)),
        )
    }
}

pub(super) fn scattered_backing(first_frame: &Val<I32>, next_entry: &Val<I32>) -> Val<I1> {
    let next_frame = first_frame.add(PAGE_BYTES);
    next_entry
        .and(FRAME_MASK)
        .ne(&next_frame)
        .or(next_frame.unsigned().lt(first_frame))
}

pub(super) fn crosses_page(start: &Val<I32>, bytes: u32) -> Val<I1> {
    if bytes == 1 {
        return false.into();
    }
    start.and(PAGE_OFFSET).unsigned().ge(PAGE_BYTES - bytes + 1)
}

/// Combines a backing frame with a linear page offset. Low backing bits are
/// ignored, so either a page-table entry or a resolved address can supply it.
pub(super) fn physical_address(backing: &Val<I32>, address: &Val<I32>) -> Val<I32> {
    backing.and(FRAME_MASK).or(address.and(PAGE_OFFSET))
}
