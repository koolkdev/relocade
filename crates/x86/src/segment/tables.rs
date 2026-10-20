//! Virtual descriptor storage owned by the host execution environment.

use crate::{Exception, SegmentRegister};

use super::{Descriptor, Segment, SegmentDescriptor, SegmentDescriptorInfo};

/// Host-managed global and local descriptor slots, addressed by x86 selectors.
/// Bits 15:3 select an entry, bit 2 selects the local table, and bits 1:0 are
/// RPL and do not choose a slot. Empty slots fail resolution with #GP.
///
/// The host chooses selector values and the descriptor view for the current
/// thread, including thread-specific entries such as Win32 FS. This collection
/// does not emulate GDTR/LDTR, guest descriptor-table memory, or Windows allocation
/// APIs. Changing an entry never changes a CPU's already-loaded segment cache.
///
/// ```
/// use relocade_x86::{
///     DescriptorTables, Exception, Segment, SegmentDefaultSize, SegmentDescriptor,
///     SegmentDescriptorKind, SegmentLimit,
/// };
///
/// let mut tables = DescriptorTables::default();
/// tables.insert(0x27, SegmentDescriptor::new(
///     0x4000, SegmentLimit::bytes(0xffff).unwrap(),
///     SegmentDescriptorKind::Data { writable: true, expand_down: false },
///     SegmentDefaultSize::Bits32,
/// ));
/// let ds = tables.resolve_user_segment(Segment::Ds, 0x27)?;
/// assert_eq!((ds.selector, ds.base, ds.limit), (0x27, 0x4000, 0xffff));
/// # Ok::<(), Exception>(())
/// ```
#[derive(Debug, Default)]
pub struct DescriptorTables {
    tables: [Vec<Option<SegmentDescriptor>>; 2],
}

impl DescriptorTables {
    /// Inspects a slot without performing segment-load checks. Even the reserved
    /// global slot zero can be inspected; selectors 0..=3 always resolve as null.
    pub fn get(&self, selector: u16) -> Option<&SegmentDescriptor> {
        let (table, index) = slot(selector);
        self.tables[table].get(index).and_then(Option::as_ref)
    }

    /// Installs a descriptor and returns any previous entry. The selector's RPL
    /// does not affect which slot is changed. Existing loaded caches are untouched.
    pub fn insert(
        &mut self,
        selector: u16,
        descriptor: SegmentDescriptor,
    ) -> Option<SegmentDescriptor> {
        let (table, index) = slot(selector);
        let entries = &mut self.tables[table];
        if index >= entries.len() {
            entries.resize(index + 1, None);
        }
        entries[index].replace(descriptor)
    }

    /// Removes a descriptor and returns it, without changing loaded caches.
    pub fn remove(&mut self, selector: u16) -> Option<SegmentDescriptor> {
        let (table, index) = slot(selector);
        self.tables[table].get_mut(index).and_then(Option::take)
    }

    /// Packs both tables, retaining empty slots as zero descriptors. Each table
    /// has at least one slot, so its architectural limit is representable.
    pub fn to_packed_tables(&self) -> [Vec<u8>; 2] {
        std::array::from_fn(|table| {
            let entries = &self.tables[table];
            (0..entries.len().max(1))
                .flat_map(|index| {
                    entries
                        .get(index)
                        .copied()
                        .flatten()
                        .map(Descriptor::from)
                        .unwrap_or_default()
                        .to_bytes()
                })
                .collect()
        })
    }

    /// Queries the current table for LAR/LSL and VERR/VERW at CPL=3. Null selectors,
    /// missing slots and privilege-inaccessible descriptors are not visible.
    /// Presence affects the reported rights, not visibility or read/write permission.
    /// Loaded caches are untouched. Only code/data descriptors are represented;
    /// all visible entries are eligible for both LAR and LSL.
    pub fn query_user_segment_descriptor(&self, selector: u16) -> SegmentDescriptorInfo {
        if selector & !3 == 0 {
            return SegmentDescriptorInfo::default();
        }
        self.get(selector)
            .map(SegmentDescriptor::query_user)
            .unwrap_or_default()
    }

    /// Resolves a segment descriptor at CPL=3 without changing CPU or table state.
    /// DS/ES/FS/GS and SS follow protected-mode data/stack load checks. CS resolves
    /// a direct code descriptor for same-privilege far CALL/JMP and sets its RPL
    /// to three. It does not permit MOV to CS, implement RET/IRET selector checks,
    /// or execute a far transfer.
    ///
    /// The caller must validate any remaining instruction effects, including a
    /// control-transfer target, before committing the returned cache. A failed
    /// load leaves the old segment and other instruction state intact. Successful
    /// loads can invalidate the current compilation profile or snapshot context;
    /// execution must leave that context before using broken assumptions.
    ///
    /// This is protected-mode user-level resolution regardless of CS.D. Real-mode
    /// loading, privilege transitions, gates and SS interrupt inhibition belong to
    /// their eventual execution paths. Descriptor privilege/type checks precede
    /// presence checks. No guest page access or descriptor accessed-bit write occurs.
    /// Errors are shared [`Exception`] values: #GP for invalid loads, #SS for a
    /// non-present SS, and #NP for other non-present segments. Error codes describe
    /// software-initiated loads, clearing RPL bits and retaining table/index bits.
    pub fn resolve_user_segment(
        &self,
        destination: Segment,
        selector: u16,
    ) -> Result<SegmentRegister, Exception> {
        self.get(selector)
            .copied()
            .map(Descriptor::from)
            .unwrap_or_default()
            .resolve_user_segment(destination, selector)
    }
}

fn slot(selector: u16) -> (usize, usize) {
    (usize::from(selector & 4 != 0), usize::from(selector >> 3))
}

#[cfg(test)]
mod tests;
