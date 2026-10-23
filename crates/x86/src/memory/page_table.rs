//! Host-managed page-table format for protected-mode guest memory.
//!
//! The `relocade.machine` memory holds one four-byte entry per 4-KiB linear page.
//! Each entry contains a backing-page offset and access permissions, in little
//! endian order. These are the engine's mappings, not guest CR3 structures.
//! Hosts can write individual entries without rebuilding the table. Apply updates
//! between guest execution calls; generated code can retain translations during a call.
//!
//! ```
//! use relocade_x86::page_table;
//!
//! # let mut table = vec![0; page_table::BYTE_LEN as usize];
//! // Map the page at guest address 00400000h to backing offset 00020000h.
//! let offset = page_table::entry_offset(0x0040_0000) as usize;
//! let entry = page_table::encode_entry(0x0002_0000, true).unwrap();
//! table[offset..offset + entry.len()].copy_from_slice(&entry);
//!
//! // Unmap the same page.
//! table[offset..offset + entry.len()].copy_from_slice(&page_table::UNMAPPED_ENTRY);
//! ```

mod cache;
#[cfg(test)]
mod tests;

pub(crate) use cache::{PageCache, PageCacheInputs};

use relocade_compiler::{BlockBuilder, BuildError, Mem, Program, TypedFunc, Val, I1, I32};

const PAGE_SHIFT: u32 = 12;
const ENTRY_SHIFT: u32 = 2;

/// Number of guest bytes covered by one table entry.
pub const PAGE_BYTES: u32 = 1 << PAGE_SHIFT;
/// Table size in bytes, covering the entire 32-bit linear address space.
pub const BYTE_LEN: u32 = 1 << (32 - PAGE_SHIFT + ENTRY_SHIFT);
/// Bytes to write at an entry's offset to unmap its page.
pub const UNMAPPED_ENTRY: [u8; 4] = [0; 4];

const PAGE_OFFSET: u32 = PAGE_BYTES - 1;
pub(super) const FRAME_MASK: u32 = !PAGE_OFFSET;
pub(super) const PRESENT: u32 = 1;
pub(super) const WRITABLE: u32 = 2;

/// Byte offset of the entry for the page containing `linear_address`.
pub const fn entry_offset(linear_address: u32) -> u32 {
    (linear_address >> PAGE_SHIFT) << ENTRY_SHIFT
}

/// Encodes a present page backed at a 4-KiB-aligned byte offset.
/// Present pages allow reads and instruction fetches; `writable` permits writes.
/// Returns `None` if `backing_offset` is not page aligned.
pub const fn encode_entry(backing_offset: u32, writable: bool) -> Option<[u8; 4]> {
    if backing_offset & PAGE_OFFSET != 0 {
        return None;
    }
    Some((backing_offset | PRESENT | if writable { WRITABLE } else { 0 }).to_le_bytes())
}

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
            entries: program.import_memory(crate::abi::PAGE_TABLE),
        }
    }

    pub(super) fn entry(
        self,
        body: &mut BlockBuilder<'_>,
        address: &Val<I32>,
    ) -> Result<Val<I32>, BuildError> {
        let index = address.unsigned().shr(PAGE_SHIFT).shl(ENTRY_SHIFT);
        body.load_at::<I32>(self.entries, index, 0)
    }

    /// Resolves every touched page of a nonempty span. The result carries its
    /// first frame and permissions, plus scattered backing, or the first denial.
    pub(super) fn range_resolver(
        self,
        program: &mut Program,
    ) -> TypedFunc<fn(u32, u32, u32, u32) -> u32> {
        program.function::<fn(u32, u32, u32, u32) -> u32>(
            move |mut body, (start, last_byte_offset, first_entry, required)| {
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
            },
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
