//! An 80-by-25 color text screen stored in guest memory.

use relocade_machine::{Memory, ReadMemory};

/// Physical start of color text memory.
pub const TEXT_MEMORY_START: u32 = 0xb8000;
/// Four 4-KiB pages, matching the PC's 16-KiB color text aperture.
pub const TEXT_MEMORY_BYTES: u32 = 0x4000;

pub(super) const PAGE_BYTES: u32 = 0x1000;
pub(super) const MODE: u32 = 0x449;
pub(super) const COLUMNS: u32 = 0x44a;
pub(super) const PAGE_SIZE: u32 = 0x44c;
pub(super) const PAGE_OFFSET: u32 = 0x44e;
pub(super) const CURSORS: u32 = 0x450;
pub(super) const CURSOR_SHAPE: u32 = 0x460;
pub(super) const ACTIVE_PAGE: u32 = 0x462;
const CRTC_PORT: u32 = 0x463;
const LAST_ROW: u32 = 0x484;
const DEFAULT_ATTRIBUTE: u8 = 7;
const CHARACTER_HEIGHT: u16 = 8;

/// One text-memory cell. Attribute bits encode foreground, background and blink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextCell {
    pub character: u8,
    pub attribute: u8,
}

/// Cursor position and scan-line shape from the BIOS data area.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextCursor {
    pub row: u8,
    pub column: u8,
    /// Full BIOS CH byte, including cursor visibility/mode bits 5–6.
    pub start_scan_line: u8,
    /// BIOS CL byte.
    pub end_scan_line: u8,
}

/// An owned snapshot for a host renderer. Guest text memory is authoritative;
/// reading a screen does not consume output or cache a separate display state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextScreen {
    pub cells: [TextCell; Self::COLUMNS * Self::ROWS],
    pub cursor: TextCursor,
    pub page: u8,
}

impl TextScreen {
    pub const COLUMNS: usize = 80;
    pub const ROWS: usize = 25;

    /// Samples the active page and cursor. When execution runs concurrently,
    /// individual cells and cursor metadata may reflect different moments.
    pub fn read(memory: &(impl ReadMemory + ?Sized)) -> Self {
        let page = active_page(memory);
        let mut bytes = [0; Self::COLUMNS * Self::ROWS * 2];
        memory
            .read(TEXT_MEMORY_START + u32::from(page) * PAGE_BYTES, &mut bytes)
            .expect("text memory");
        let [column, row] = cursor_position(memory, page).to_le_bytes();
        let [end_scan_line, start_scan_line] = memory
            .read_u16(CURSOR_SHAPE)
            .expect("video cursor shape")
            .to_le_bytes();
        Self {
            cells: std::array::from_fn(|index| TextCell {
                character: bytes[index * 2],
                attribute: bytes[index * 2 + 1],
            }),
            cursor: TextCursor {
                row,
                column,
                start_scan_line,
                end_scan_line,
            },
            page,
        }
    }
}

/// Writes guest-encoded bytes to the active page using BIOS teletype behavior.
/// CR, LF and backspace move the cursor; other glyphs retain cell attributes.
/// This updates guest video memory and BIOS cursor data without a host output stream.
pub fn write_text(memory: &mut Memory<'_>, bytes: &[u8]) {
    let page = active_page(memory);
    for &byte in bytes {
        teletype(memory, page, byte);
    }
}

pub(super) fn active_page(memory: &(impl ReadMemory + ?Sized)) -> u8 {
    // The initial color text aperture has four pages.
    memory.read_u8(ACTIVE_PAGE).expect("active video page") & 3
}

pub(super) fn valid_page(page: u8) -> bool {
    u32::from(page) < TEXT_MEMORY_BYTES / PAGE_BYTES
}

pub(super) fn select_page(memory: &mut Memory<'_>, page: u8) {
    memory
        .write_u8(ACTIVE_PAGE, page)
        .expect("active video page");
    memory
        .write_u16(PAGE_OFFSET, u16::from(page) * PAGE_BYTES as u16)
        .expect("video page offset");
}

pub(super) fn cursor_position(memory: &(impl ReadMemory + ?Sized), page: u8) -> u16 {
    memory
        .read_u16(CURSORS + u32::from(page) * 2)
        .expect("video cursor")
}

pub(super) fn set_cursor_position(memory: &mut Memory<'_>, page: u8, position: u16) {
    memory
        .write_u16(CURSORS + u32::from(page) * 2, position)
        .expect("video cursor");
}

fn cell_address(page: u8, row: u8, column: u8) -> u32 {
    TEXT_MEMORY_START
        + u32::from(page) * PAGE_BYTES
        + (u32::from(row) * TextScreen::COLUMNS as u32 + u32::from(column)) * 2
}

pub(super) fn read_cell(memory: &mut Memory<'_>, page: u8) -> [u8; 2] {
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let mut cell = [0; 2];
    if usize::from(row) < TextScreen::ROWS && usize::from(column) < TextScreen::COLUMNS {
        memory
            .read(cell_address(page, row, column), &mut cell)
            .expect("text cell");
    }
    cell
}

pub(super) fn write_repeated(
    memory: &mut Memory<'_>,
    page: u8,
    character: u8,
    attribute: Option<u8>,
    count: u16,
) {
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let start = u16::from(page) * PAGE_BYTES as u16
        + (u16::from(row) * TextScreen::COLUMNS as u16 + u16::from(column)) * 2;
    // BIOS repeat writes advance a 16-bit video offset; the visible page edge
    // does not stop them or move the cursor.
    for index in 0..count {
        let offset = start.wrapping_add(index.wrapping_mul(2));
        let address = TEXT_MEMORY_START + u32::from(offset);
        let cell = [character, attribute.unwrap_or(0)];
        memory
            .write(address, &cell[..if attribute.is_some() { 2 } else { 1 }])
            .expect("text cell");
    }
}

pub(super) struct Window {
    pub top: u8,
    pub left: u8,
    pub bottom: u8,
    pub right: u8,
}

pub(super) enum ScrollDirection {
    Up,
    Down,
}

pub(super) fn scroll(
    memory: &mut Memory<'_>,
    page: u8,
    window: Window,
    lines: u8,
    attribute: u8,
    direction: ScrollDirection,
) {
    let bottom = window.bottom.min((TextScreen::ROWS - 1) as u8);
    let right = window.right.min((TextScreen::COLUMNS - 1) as u8);
    if window.top > bottom || window.left > right {
        return;
    }
    let height = bottom - window.top + 1;
    let count = if lines == 0 {
        height
    } else {
        lines.min(height)
    };
    let width_bytes = usize::from(right - window.left + 1) * 2;
    let mut row_bytes = [0; TextScreen::COLUMNS * 2];
    for index in 0..height {
        let (row, source) = match direction {
            ScrollDirection::Up => {
                let row = window.top + index;
                (row, i16::from(row) + i16::from(count))
            }
            ScrollDirection::Down => {
                let row = bottom - index;
                (row, i16::from(row) - i16::from(count))
            }
        };
        let bytes = &mut row_bytes[..width_bytes];
        if source >= i16::from(window.top) && source <= i16::from(bottom) {
            memory
                .read(cell_address(page, source as u8, window.left), bytes)
                .expect("scroll source");
        } else {
            for cell in bytes.as_chunks_mut::<2>().0 {
                *cell = [b' ', attribute];
            }
        }
        memory
            .write(cell_address(page, row, window.left), bytes)
            .expect("scroll destination");
    }
}

fn teletype(memory: &mut Memory<'_>, page: u8, character: u8) {
    let [mut column, mut row] = cursor_position(memory, page).to_le_bytes();
    if usize::from(row) >= TextScreen::ROWS || usize::from(column) >= TextScreen::COLUMNS {
        return;
    }
    match character {
        7 => return, // Bell requires a future speaker device.
        8 => column = column.saturating_sub(1),
        b'\r' => column = 0,
        b'\n' => row += 1,
        _ => {
            memory
                .write_u8(cell_address(page, row, column), character)
                .expect("text character");
            column += 1;
            if usize::from(column) == TextScreen::COLUMNS {
                column = 0;
                row += 1;
            }
        }
    }
    if usize::from(row) == TextScreen::ROWS {
        row -= 1;
        // BIOS reads the fill attribute after advancing the cursor. A wrapped
        // line uses column zero; a line feed retains its previous column.
        set_cursor_position(memory, page, u16::from_le_bytes([column, row]));
        let attribute = read_cell(memory, page)[1];
        scroll(
            memory,
            page,
            Window {
                top: 0,
                left: 0,
                bottom: row,
                right: (TextScreen::COLUMNS - 1) as u8,
            },
            1,
            attribute,
            ScrollDirection::Up,
        );
    }
    set_cursor_position(memory, page, u16::from_le_bytes([column, row]));
}

pub(super) fn initialize(memory: &mut Memory<'_>) {
    memory.write_u8(MODE, 3).expect("video mode");
    memory
        .write_u16(COLUMNS, TextScreen::COLUMNS as u16)
        .expect("video columns");
    memory
        .write_u16(PAGE_SIZE, PAGE_BYTES as u16)
        .expect("video page size");
    memory.write_u16(PAGE_OFFSET, 0).expect("video page offset");
    memory.write(CURSORS, &[0; 16]).expect("video cursors");
    memory
        .write_u16(CURSOR_SHAPE, 0x0607)
        .expect("video cursor shape");
    memory.write_u8(ACTIVE_PAGE, 0).expect("video active page");
    // BIOS data: color CRTC port, last text row and character height.
    memory.write_u16(CRTC_PORT, 0x3d4).expect("CRTC port");
    let [height_lo, height_hi] = CHARACTER_HEIGHT.to_le_bytes();
    memory
        .write(
            LAST_ROW,
            &[(TextScreen::ROWS - 1) as u8, height_lo, height_hi],
        )
        .expect("video font geometry");
    let blank_page = [b' ', DEFAULT_ATTRIBUTE].repeat(PAGE_BYTES as usize / 2);
    for page in 0..TEXT_MEMORY_BYTES / PAGE_BYTES {
        memory
            .write(TEXT_MEMORY_START + page * PAGE_BYTES, &blank_page)
            .expect("text page");
    }
}
