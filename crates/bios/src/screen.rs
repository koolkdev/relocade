//! An 80-by-25 color text screen stored in guest memory.

use relocade_machine::Memory;

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

    /// Copies the active page and cursor while guest execution is stopped.
    pub fn read(memory: &mut Memory<'_>) -> Self {
        let mut page = [0];
        memory
            .read(ACTIVE_PAGE, &mut page)
            .expect("video BIOS data");
        // The initial color text aperture has four pages.
        let page = page[0] & 3;
        let mut bytes = [0; Self::COLUMNS * Self::ROWS * 2];
        memory
            .read(TEXT_MEMORY_START + u32::from(page) * PAGE_BYTES, &mut bytes)
            .expect("text memory");
        let mut position = [0; 2];
        memory
            .read(CURSORS + u32::from(page) * 2, &mut position)
            .expect("video cursor");
        let mut shape = [0; 2];
        memory
            .read(CURSOR_SHAPE, &mut shape)
            .expect("video cursor shape");
        Self {
            cells: std::array::from_fn(|index| TextCell {
                character: bytes[index * 2],
                attribute: bytes[index * 2 + 1],
            }),
            cursor: TextCursor {
                row: position[1],
                column: position[0],
                start_scan_line: shape[1],
                end_scan_line: shape[0],
            },
            page,
        }
    }
}

pub(super) fn initialize(memory: &mut Memory<'_>) {
    memory.write(MODE, &[3]).expect("video mode");
    memory
        .write(COLUMNS, &(TextScreen::COLUMNS as u16).to_le_bytes())
        .expect("video columns");
    memory
        .write(PAGE_SIZE, &(PAGE_BYTES as u16).to_le_bytes())
        .expect("video page size");
    memory
        .write(PAGE_OFFSET, &[0; 2])
        .expect("video page offset");
    memory.write(CURSORS, &[0; 16]).expect("video cursors");
    memory
        .write(CURSOR_SHAPE, &[7, 6])
        .expect("video cursor shape");
    memory.write(ACTIVE_PAGE, &[0]).expect("video active page");
    // BIOS data: color CRTC port, last text row and character height.
    memory
        .write(CRTC_PORT, &0x3d4u16.to_le_bytes())
        .expect("CRTC port");
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
