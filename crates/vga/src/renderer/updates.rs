//! Conservative pixel bounds for VRAM-only changes. State changes redraw fully.
use crate::{
    registers::{attribute, crtc, sequencer},
    DisplaySnapshot,
};
use std::ops::Range;

/// Exclusive pixel bounds, clipped to the image before encoding.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct PixelRect {
    pub left: u32,
    pub top: u32,
    pub right: u32,
    pub bottom: u32,
}

impl PixelRect {
    pub fn full(size: [u32; 2]) -> Self {
        Self {
            left: 0,
            top: 0,
            right: size[0],
            bottom: size[1],
        }
    }

    pub fn include(region: &mut Option<Self>, rect: Self) {
        if let Some(region) = region {
            region.left = region.left.min(rect.left);
            region.top = region.top.min(rect.top);
            region.right = region.right.max(rect.right);
            region.bottom = region.bottom.max(rect.bottom);
        } else {
            *region = Some(rect);
        }
    }
}

pub(super) fn memory_region(
    snapshot: &DisplaySnapshot,
    bounds: [Range<usize>; 4],
) -> Option<PixelRect> {
    // Font and other-plane changes may affect any displayed cell.
    if bounds[2..].iter().any(|range| !range.is_empty()) {
        return Some(PixelRect::full(snapshot.geometry().size));
    }
    let mut region = None;
    for bytes in bounds[..2].iter().filter(|bytes| !bytes.is_empty()) {
        if let Some(rect) = text_region(snapshot, bytes.clone()) {
            PixelRect::include(&mut region, rect);
        }
    }
    region
}

fn text_region(snapshot: &DisplaySnapshot, bytes: Range<usize>) -> Option<PixelRect> {
    let r = &snapshot.registers;
    let c = &r.crtc.values;
    let a = &r.attribute.values;
    let geometry = snapshot.geometry();
    let full = Some(PixelRect::full(geometry.size));
    let maximum_scan = c[crtc::MAXIMUM_SCAN_LINE as usize];
    let divisor = r.crtc.scanlines_per_vertical_count() as u32;
    let split = u32::from(c[crtc::LINE_COMPARE as usize])
        | (u32::from(c[crtc::OVERFLOW as usize] & 0x10) << 4)
        | (u32::from(maximum_scan & 0x40) << 3);
    let cell_width = if r.sequencer.values[sequencer::CLOCKING_MODE as usize] & 1 != 0 {
        8
    } else {
        9
    };
    let pan = a[attribute::HORIZONTAL_PANNING as usize];
    // Restrict partial decoding to linear word-addressed text. More complicated
    // addressing stays in the full GPU decoder, including graphics layouts.
    if a[attribute::MODE_CONTROL as usize] & 0x41 != 0
        || c[crtc::MODE_CONTROL as usize] & 0x6b != 0x23
        || c[crtc::UNDERLINE_LOCATION as usize] & 0x60 != 0
        || c[crtc::PRESET_ROW_SCAN as usize] & 0x7f != 0
        || (split + 1) * divisor < geometry.size[1] * geometry.scanline_step
        || (cell_width == 8 && pan & 7 != 0)
        || (cell_width == 9 && pan < 8)
    {
        return full;
    }
    let cell_height =
        (u32::from(maximum_scan & 31) + 1) * (1 << (maximum_scan >> 7)) / geometry.scanline_step;
    let rows = geometry.size[1].div_ceil(cell_height);
    let columns = u32::from(c[crtc::HORIZONTAL_DISPLAY_END as usize]) + 1;
    let stride = u32::from(c[crtc::OFFSET as usize]) * 2;
    let start = u32::from(r.crtc.display_start);
    if stride < columns || start + (rows - 1) * stride + columns > 32768 {
        return full;
    }
    let cells = bytes.start.div_ceil(2) as u32..bytes.end.div_ceil(2) as u32;
    let mut region = None;
    for row in 0..rows {
        let row_start = start + row * stride;
        let first = cells.start.max(row_start);
        let end = cells.end.min(row_start + columns);
        if first < end {
            PixelRect::include(
                &mut region,
                PixelRect {
                    left: (first - row_start) * cell_width,
                    right: (end - row_start) * cell_width,
                    top: row * cell_height,
                    bottom: ((row + 1) * cell_height).min(geometry.size[1]),
                },
            );
        }
    }
    region
}
