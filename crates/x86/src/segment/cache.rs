//! Logical fields of a loaded segment, shared by state initialization and generated code.

use super::Segment;
use relocade_compiler::{abi, Val, I16, I32};

#[derive(relocade_wasm_bindings::WasmAbi, abi::CompilerValue)]
pub(crate) struct SegmentProperties {
    pub(crate) limit: u32,
    pub(crate) usable: bool,
    pub(crate) readable: bool,
    pub(crate) writable: bool,
    pub(crate) executable: bool,
    pub(crate) expand_down: bool,
    pub(crate) big: bool,
}

impl SegmentProperties {
    pub(crate) const fn real_mode(segment: Segment) -> Self {
        let code = matches!(segment, Segment::Cs);
        Self {
            limit: u16::MAX as u32,
            usable: true,
            readable: true,
            writable: !code,
            executable: code,
            expand_down: false,
            big: false,
        }
    }
}

#[derive(relocade_wasm_bindings::WasmAbi, abi::CompilerValue)]
pub(crate) struct SegmentCache {
    pub(crate) base: u32,
    pub(crate) selector: u16,
    pub(crate) properties: SegmentProperties,
}

/// A successful selector resolution. Completion is deferred until instruction commit.
#[derive(relocade_wasm_bindings::WasmAbi, abi::CompilerValue)]
pub(crate) struct SegmentResolution {
    pub(crate) cache: SegmentCache,
    pub(crate) needs_completion: bool,
}

impl SegmentResolutionValues {
    pub(crate) fn real_mode(segment: Segment, selector: &Val<I16>) -> Self {
        let properties = SegmentProperties::real_mode(segment);
        Self {
            cache: SegmentCacheValues {
                base: selector.unsigned().extend::<I32>().shl(4),
                selector: selector.clone(),
                properties: abi::constant::<SegmentProperties>(properties),
            },
            needs_completion: false.into(),
        }
    }
}
