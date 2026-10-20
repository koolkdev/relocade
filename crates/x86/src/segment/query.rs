//! Descriptor query results, independent of loaded segment caches.

use super::{Descriptor, SegmentDescriptor};

/// Information visible at CPL=3. System descriptor types can have different
/// LAR and LSL eligibility. Presence does not determine query success.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SegmentDescriptorInfo<Bit = bool, Word = u32> {
    pub lar_valid: Bit,
    pub lsl_valid: Bit,
    pub readable: Bit,
    pub writable: Bit,
    /// LAR result; undefined bits 19:16 are chosen as zero.
    pub access_rights: Word,
    /// Inclusive effective byte limit returned by LSL.
    pub limit: Word,
}

impl SegmentDescriptor {
    pub(super) fn query_user(&self) -> SegmentDescriptorInfo {
        Descriptor::from(*self).query_user()
    }
}
