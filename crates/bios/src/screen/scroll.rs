//! Character-window clipping and overlap-safe row order, shared by all BIOS layouts.
use super::geometry::Geometry;
use relocade_machine::MemoryView;

pub(crate) struct Window {
    pub top: u8,
    pub left: u8,
    pub bottom: u8,
    pub right: u8,
}

#[derive(Clone, Copy)]
pub(crate) enum ScrollDirection {
    Up,
    Down,
}

pub(crate) struct Scroll {
    pub left: u16,
    pub width: u16,
    top: u16,
    bottom: u16,
    count: u16,
    direction: ScrollDirection,
}

impl Scroll {
    pub fn new(
        window: Window,
        lines: u8,
        direction: ScrollDirection,
        columns: u16,
        rows: u16,
    ) -> Option<Self> {
        if columns == 0 || rows == 0 {
            return None;
        }
        let top = u16::from(window.top);
        let left = u16::from(window.left);
        let bottom = u16::from(window.bottom).min(rows - 1);
        let right = u16::from(window.right).min(columns - 1);
        if top > bottom || left > right {
            return None;
        }
        let height = bottom - top + 1;
        let count = if lines == 0 {
            height
        } else {
            u16::from(lines).min(height)
        };
        Some(Self {
            left,
            width: right - left + 1,
            top,
            bottom,
            count,
            direction,
        })
    }

    pub fn rows(&self) -> impl Iterator<Item = (u16, Option<u16>)> + '_ {
        (0..=self.bottom - self.top).map(|index| {
            let (row, source) = match self.direction {
                ScrollDirection::Up => {
                    let row = self.top + index;
                    (row, row + self.count)
                }
                ScrollDirection::Down => {
                    let row = self.bottom - index;
                    (row, row.wrapping_sub(self.count))
                }
            };
            (
                row,
                (self.top..=self.bottom).contains(&source).then_some(source),
            )
        })
    }

    pub(super) fn text(
        &self,
        memory: &mut MemoryView<'_>,
        geometry: &Geometry,
        page: u8,
        attribute: u8,
    ) {
        // INT 10h window columns are bytes, so at most 256 cells can be selected.
        let mut row_bytes = [0; 256 * 2];
        let bytes = &mut row_bytes[..usize::from(self.width) * 2];
        for (row, source) in self.rows() {
            if let Some(source) = source {
                memory
                    .read(geometry.cell_address(page, source, self.left), bytes)
                    .expect("scroll source");
            } else {
                for cell in bytes.as_chunks_mut::<2>().0 {
                    *cell = [b' ', attribute];
                }
            }
            memory
                .write(geometry.cell_address(page, row, self.left), bytes)
                .expect("scroll destination");
        }
    }
}
