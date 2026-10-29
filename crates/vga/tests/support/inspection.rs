// Immediate device inspection for tests; rendering consumes physical snapshots.
#![allow(dead_code)]
use relocade_vga::DisplaySnapshot;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cell {
    pub character: u8,
    pub attribute: u8,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Cursor {
    pub address: u16,
    pub start_scan_line: u8,
    pub end_scan_line: u8,
}
#[derive(Debug, Eq, PartialEq)]
pub struct Inspection {
    pub cells: Vec<Cell>,
    pub cursor: Cursor,
    pub start_address: u16,
}

pub fn inspect(snapshot: &DisplaySnapshot) -> Inspection {
    let text = snapshot.text_display();
    let cells = (0..text.layout.size[1].div_ceil(text.layout.glyph_height))
        .flat_map(|row| {
            let text = &text;
            (0..text.layout.columns).map(move |column| {
                let line = text.layout.scanline(row * text.layout.glyph_height);
                let address = usize::from(
                    text.layout
                        .address(line.counter.wrapping_add(column as u16), line.glyph_row),
                );
                Cell {
                    character: snapshot.plane(0)[address],
                    attribute: snapshot.plane(1)[address],
                }
            })
        })
        .collect();
    Inspection {
        cells,
        cursor: Cursor {
            address: text.cursor_address,
            start_scan_line: text.cursor_start,
            end_scan_line: text.cursor_end,
        },
        start_address: text.layout.start_address,
    }
}
