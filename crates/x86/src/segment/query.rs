//! Nonfaulting queries of the host's protected-mode code/data descriptors.

use super::{SegmentDefaultSize, SegmentDescriptor, SegmentDescriptorKind};

/// Descriptor information visible at CPL=3, independent of presence and loaded
/// caches. `visible` admits LAR/LSL; read/write permission admits VERR/VERW.
/// Execute-only code can be visible without either permission. Values are zero
/// when visibility is denied. Neither visibility nor permission guarantees that
/// a segment load or memory access succeeds.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SegmentDescriptorInfo<Bit = bool, Word = u32> {
    pub visible: Bit,
    pub readable: Bit,
    pub writable: Bit,
    /// LAR's 32-bit result; undefined bits 19:16 are chosen as zero.
    pub access_rights: Word,
    /// Inclusive effective byte limit, as returned by LSL with a 32-bit destination.
    pub limit: Word,
}

impl SegmentDescriptor {
    pub(super) fn query_user(&self) -> SegmentDescriptorInfo {
        if !self.user_visible() {
            return SegmentDescriptorInfo::default();
        }
        let (readable, writable, kind) = match self.kind {
            SegmentDescriptorKind::Data {
                writable,
                expand_down,
            } => (
                true,
                writable,
                (u32::from(expand_down) << 2) | (u32::from(writable) << 1),
            ),
            SegmentDescriptorKind::Code {
                readable,
                conforming,
            } => (
                readable,
                false,
                8 | (u32::from(conforming) << 2) | (u32::from(readable) << 1),
            ),
        };
        // A=1 and S=1 are invariants of the host descriptor model. L=0 because
        // these are 16/32-bit descriptors. LAR leaves bits 19:16 undefined; choose 0.
        let access_rights = ((kind | 1) << 8)
            | (1 << 12)
            | ((self.dpl as u32) << 13)
            | (u32::from(self.present) << 15)
            | (u32::from(self.available) << 20)
            | (u32::from(self.default_size == SegmentDefaultSize::Bits32) << 22)
            | (u32::from(self.limit.is_page_granular()) << 23);
        SegmentDescriptorInfo {
            visible: true,
            readable,
            writable,
            access_rights,
            limit: self.limit.effective(),
        }
    }
}
