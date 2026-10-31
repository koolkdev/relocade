//! A conservative linear-text fast path. VGA pixel interpretation stays in WGSL.
use super::PixelRect;
use crate::{
    registers::{attribute, crtc, sequencer},
    DisplaySnapshot,
};
use std::ops::Range;

pub(super) struct TextLayout {
    columns: u32,
    rows: u32,
    cell_width: u32,
    cell_height: u32,
    row_scale: u32,
    glyph_height: u32,
    image_height: u32,
    start: u32,
    stride: u32,
}

impl TextLayout {
    pub fn new(snapshot: &DisplaySnapshot) -> Option<Self> {
        let r = &snapshot.registers;
        let c = &r.crtc.values;
        let a = &r.attribute.values;
        let mode = c[crtc::MODE_CONTROL as usize];
        let underline = c[crtc::UNDERLINE_LOCATION as usize];
        let geometry = snapshot.geometry();
        let maximum_scan = c[crtc::MAXIMUM_SCAN_LINE as usize];
        let divisor = r.crtc.scanlines_per_vertical_count() as u32;
        let split = u32::from(c[crtc::LINE_COMPARE as usize])
            | (u32::from(c[crtc::OVERFLOW as usize] & 0x10) << 4)
            | (u32::from(maximum_scan & 0x40) << 3);
        // Word addresses without rotation, wrap, row substitution, panning,
        // overlapping rows, splits or count division have one cell per address.
        if a[attribute::MODE_CONTROL as usize] & 0x41 != 0
            || mode & 0x6b != 0x23
            || underline & 0x60 != 0
            || c[crtc::PRESET_ROW_SCAN as usize] & 0x7f != 0
            || (split + 1) * divisor < geometry.size[1] * geometry.scanline_step
        {
            return None;
        }
        let cell_width = if r.sequencer.values[sequencer::CLOCKING_MODE as usize] & 1 != 0 {
            8
        } else {
            9
        };
        let pan = a[attribute::HORIZONTAL_PANNING as usize];
        if (cell_width == 8 && pan & 7 != 0) || (cell_width == 9 && pan < 8) {
            return None;
        }
        let glyph_height = u32::from(maximum_scan & 31) + 1;
        let row_scale = (1 << (maximum_scan >> 7)) / geometry.scanline_step;
        let cell_height = glyph_height * row_scale;
        let rows = geometry.size[1].div_ceil(cell_height);
        let columns = u32::from(c[crtc::HORIZONTAL_DISPLAY_END as usize]) + 1;
        let stride = u32::from(c[crtc::OFFSET as usize]) * 2;
        let start = u32::from(r.crtc.display_start);
        if stride < columns || start + (rows - 1) * stride + columns > 32768 {
            return None;
        }
        Some(Self {
            columns,
            rows,
            cell_width,
            cell_height,
            row_scale,
            glyph_height,
            image_height: geometry.size[1],
            start,
            stride,
        })
    }

    fn include_cells(&self, region: &mut Option<PixelRect>, row: u32, columns: Range<u32>) {
        PixelRect::include(
            region,
            PixelRect {
                left: columns.start * self.cell_width,
                right: columns.end * self.cell_width,
                top: row * self.cell_height,
                bottom: ((row + 1) * self.cell_height).min(self.image_height),
            },
        );
    }

    pub fn include_bytes(&self, region: &mut Option<PixelRect>, bytes: Range<usize>) {
        let cells = bytes.start.div_ceil(2) as u32..bytes.end.div_ceil(2) as u32;
        for row in 0..self.rows {
            let start = self.start + row * self.stride;
            let first = cells.start.max(start);
            let end = cells.end.min(start + self.columns);
            if first < end {
                self.include_cells(region, row, first - start..end - start);
            }
        }
    }

    pub fn include_blink_and_cursor(
        &self,
        region: &mut Option<PixelRect>,
        previous: &DisplaySnapshot,
        current: &DisplaySnapshot,
    ) {
        let old = &previous.registers.crtc.values;
        let new = &current.registers.crtc.values;
        let cursor_changed = [
            crtc::CURSOR_START,
            crtc::CURSOR_END,
            crtc::CURSOR_ADDRESS_HIGH,
            crtc::CURSOR_ADDRESS_LOW,
        ]
        .iter()
        .any(|&index| old[index as usize] != new[index as usize]);
        let blink_changed = previous.blink_on != current.blink_on;
        if cursor_changed || blink_changed {
            self.include_cursor(region, previous);
            self.include_cursor(region, current);
        }
        if blink_changed
            && current.registers.attribute.values[attribute::MODE_CONTROL as usize] & 8 != 0
        {
            // Locate blink-enabled cells, not changed bytes. Writes were already
            // tracked at MMIO; this bounded attribute lookup runs only on a phase change.
            for row in 0..self.rows {
                let start = self.start + row * self.stride;
                for column in 0..self.columns {
                    if current.plane(1)[((start + column) * 2) as usize] & 0x80 != 0 {
                        self.include_cells(region, row, column..column + 1);
                    }
                }
            }
        }
    }

    fn include_cursor(&self, region: &mut Option<PixelRect>, snapshot: &DisplaySnapshot) {
        let c = &snapshot.registers.crtc.values;
        let start = c[crtc::CURSOR_START as usize];
        let end = c[crtc::CURSOR_END as usize];
        let first_row = u32::from(start & 31);
        let last_row = u32::from(end & 31).min(self.glyph_height - 1);
        if !snapshot.blink_on || start & 0x20 != 0 || first_row > last_row {
            return;
        }
        let address = u32::from(u16::from_be_bytes([
            c[crtc::CURSOR_ADDRESS_HIGH as usize],
            c[crtc::CURSOR_ADDRESS_LOW as usize],
        ]));
        let Some(offset) = address.checked_sub(self.start) else {
            return;
        };
        let row = offset / self.stride;
        let column = offset % self.stride + u32::from((end >> 5) & 3);
        if row < self.rows && column < self.columns {
            let top = row * self.cell_height + first_row * self.row_scale;
            let bottom =
                (row * self.cell_height + (last_row + 1) * self.row_scale).min(self.image_height);
            if top < bottom {
                PixelRect::include(
                    region,
                    PixelRect {
                        left: column * self.cell_width,
                        right: (column + 1) * self.cell_width,
                        top,
                        bottom,
                    },
                );
            }
        }
    }
}
