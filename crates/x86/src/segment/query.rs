//! Descriptor query results, independent of loaded segment caches.

use super::{Descriptor, SegmentDescriptor};

/// Information visible at CPL=3. System descriptor types can have different
/// LAR and LSL eligibility. Presence does not determine query success.
#[derive(
    relocade_wasm_bindings::WasmAbi,
    relocade_compiler::abi::CompilerValue,
    Clone,
    Copy,
    Debug,
    Default,
    Eq,
    PartialEq,
)]
pub struct SegmentDescriptorInfo {
    pub lar_valid: bool,
    pub lsl_valid: bool,
    pub readable: bool,
    pub writable: bool,
    /// LAR result; undefined bits 19:16 are chosen as zero.
    pub access_rights: u32,
    /// Inclusive effective byte limit returned by LSL.
    pub limit: u32,
}

impl SegmentDescriptor {
    pub(super) fn query_user(&self) -> SegmentDescriptorInfo {
        Descriptor::from(*self).query_user()
    }
}
