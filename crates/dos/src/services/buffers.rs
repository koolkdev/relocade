//! DOS services choose whether a transfer stops at or wraps around a segment end.

use relocade_machine::Memory;
use relocade_x86::StoredSegment;

const SEGMENT_BYTES: usize = 0x10000;

/// AH=3Fh/40h transfer only the part of DS:DX's buffer that fits in its segment.
pub(super) fn transfer_count(offset: u16, count: u16) -> usize {
    usize::from(count).min(SEGMENT_BYTES - usize::from(offset))
}

/// DOS byte-string copies advance a 16-bit offset, wrapping within the segment.
pub(super) fn write_wrapping(
    memory: &mut Memory<'_>,
    segment: &StoredSegment,
    mut offset: u16,
    mut bytes: &[u8],
) {
    while !bytes.is_empty() {
        let count = bytes.len().min(SEGMENT_BYTES - usize::from(offset));
        memory
            .write_segment(segment, u32::from(offset), &bytes[..count])
            .expect("real-mode string span");
        bytes = &bytes[count..];
        offset = 0;
    }
}
