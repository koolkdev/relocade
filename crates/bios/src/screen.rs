//! BIOS text-page operations and firmware display data.

use relocade_machine::{IoBus, MemoryView};
use relocade_vga::{
    registers::{
        attribute, crtc, sequencer, text_mode, ATTRIBUTE_ADDRESS_PORT, COLOR_INPUT_STATUS_1_PORT,
        CRTC_INDEX_PORT, DAC_DATA_PORT, DAC_MASK_PORT, DAC_WRITE_ADDRESS_PORT, GRAPHICS_INDEX_PORT,
        MISC_OUTPUT_WRITE_PORT, SEQUENCER_INDEX_PORT,
    },
    TextScreen, TEXT_MEMORY_BYTES, TEXT_MEMORY_START,
};

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

/// Writes guest-encoded bytes to the active page using BIOS teletype behavior.
/// CR, LF and backspace move the cursor; other glyphs retain cell attributes.
/// This updates guest video memory and BIOS cursor data without a host output stream.
pub fn write_text(memory: &mut MemoryView<'_>, io: &IoBus, bytes: &[u8]) {
    let page = active_page(memory);
    for &byte in bytes {
        teletype(memory, io, page, byte);
    }
}

pub(super) fn active_page(memory: &MemoryView<'_>) -> u8 {
    memory.read_u8(ACTIVE_PAGE).expect("active video page")
}

pub(super) fn valid_page(page: u8) -> bool {
    u32::from(page) < TEXT_MEMORY_BYTES / PAGE_BYTES
}

pub(super) fn select_page(memory: &mut MemoryView<'_>, io: &IoBus, page: u8) {
    memory
        .write_u8(ACTIVE_PAGE, page)
        .expect("active video page");
    memory
        .write_u16(PAGE_OFFSET, u16::from(page) * PAGE_BYTES as u16)
        .expect("video page offset");
    write_crtc_word(
        io,
        crtc::START_ADDRESS_HIGH,
        u16::from(page) * (PAGE_BYTES / 2) as u16,
    );
    update_cursor(memory, io, page);
}

pub(super) fn cursor_position(memory: &MemoryView<'_>, page: u8) -> u16 {
    memory
        .read_u16(CURSORS + u32::from(page) * 2)
        .expect("video cursor")
}

pub(super) fn set_cursor_position(
    memory: &mut MemoryView<'_>,
    io: &IoBus,
    page: u8,
    position: u16,
) {
    memory
        .write_u16(CURSORS + u32::from(page) * 2, position)
        .expect("video cursor");
    if page == active_page(memory) {
        update_cursor(memory, io, page);
    }
}

pub(super) fn set_cursor_shape(memory: &mut MemoryView<'_>, io: &IoBus, shape: u16) {
    memory
        .write_u16(CURSOR_SHAPE, shape)
        .expect("video cursor shape");
    let [start, end] = shape.to_be_bytes();
    write_indexed(io, CRTC_INDEX_PORT, crtc::CURSOR_START, start);
    write_indexed(io, CRTC_INDEX_PORT, crtc::CURSOR_END, end);
}

fn update_cursor(memory: &MemoryView<'_>, io: &IoBus, page: u8) {
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let position = u16::from(page)
        .wrapping_mul((PAGE_BYTES / 2) as u16)
        .wrapping_add(u16::from(row) * TextScreen::COLUMNS as u16 + u16::from(column));
    write_crtc_word(io, crtc::CURSOR_ADDRESS_HIGH, position);
}

fn write_indexed(io: &IoBus, index_port: u16, register: u8, value: u8) {
    io.write_port(
        index_port,
        2,
        u32::from(u16::from_le_bytes([register, value])),
    );
}

fn write_crtc_word(io: &IoBus, register: u8, value: u16) {
    let [high, low] = value.to_be_bytes();
    write_indexed(io, CRTC_INDEX_PORT, register, high);
    write_indexed(io, CRTC_INDEX_PORT, register + 1, low);
}

fn cell_address(page: u8, row: u8, column: u8) -> u32 {
    TEXT_MEMORY_START
        + u32::from(page) * PAGE_BYTES
        + (u32::from(row) * TextScreen::COLUMNS as u32 + u32::from(column)) * 2
}

pub(super) fn read_cell(memory: &MemoryView<'_>, page: u8) -> [u8; 2] {
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
    memory: &mut MemoryView<'_>,
    page: u8,
    character: u8,
    attribute: Option<u8>,
    count: u16,
) {
    let [column, row] = cursor_position(memory, page).to_le_bytes();
    let start = (u16::from(page) * PAGE_BYTES as u16)
        .wrapping_add((u16::from(row) * TextScreen::COLUMNS as u16 + u16::from(column)) * 2);
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
    memory: &mut MemoryView<'_>,
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

fn teletype(memory: &mut MemoryView<'_>, io: &IoBus, page: u8, character: u8) {
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
        set_cursor_position(memory, io, page, u16::from_le_bytes([column, row]));
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
    set_cursor_position(memory, io, page, u16::from_le_bytes([column, row]));
}

fn program_text_mode(io: &IoBus) {
    io.write_port(MISC_OUTPUT_WRITE_PORT, 1, u32::from(text_mode::MISC_OUTPUT));
    for &(register, value) in text_mode::SEQUENCER_REGISTERS {
        if register != sequencer::RESET {
            write_indexed(io, SEQUENCER_INDEX_PORT, register, value);
        }
    }
    for &(register, value) in text_mode::GRAPHICS_REGISTERS {
        write_indexed(io, GRAPHICS_INDEX_PORT, register, value);
    }
    write_indexed(
        io,
        CRTC_INDEX_PORT,
        crtc::VERTICAL_RETRACE_END,
        text_mode::VERTICAL_RETRACE_END & !crtc::PROTECT_TIMING_REGISTERS,
    );
    for &(register, value) in text_mode::CRTC_REGISTERS {
        write_indexed(io, CRTC_INDEX_PORT, register, value);
    }
    io.read_port(COLOR_INPUT_STATUS_1_PORT, 1);
    for (index, value) in text_mode::ATTRIBUTE_PALETTE.into_iter().enumerate() {
        io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, index as u32);
        io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(value));
    }
    for &(register, value) in text_mode::ATTRIBUTE_REGISTERS {
        io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(register));
        io.write_port(ATTRIBUTE_ADDRESS_PORT, 1, u32::from(value));
    }
    io.write_port(
        ATTRIBUTE_ADDRESS_PORT,
        1,
        u32::from(attribute::PALETTE_ADDRESS_SOURCE),
    );
    io.write_port(DAC_MASK_PORT, 1, 0xff);
    io.write_port(DAC_WRITE_ADDRESS_PORT, 1, 0);
    for color in text_mode::DAC_PALETTE {
        for component in color {
            io.write_port(DAC_DATA_PORT, 1, u32::from(component));
        }
    }
}

pub(super) fn initialize(memory: &mut MemoryView<'_>, io: &IoBus) {
    // Hold the sequencer in reset while restoring the preset and text pages.
    // CPU addressing and write masks must be restored before clearing memory.
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::SYNCHRONOUS_RESET,
    );
    program_text_mode(io);
    memory.write_u8(MODE, 3).expect("video mode");
    memory
        .write_u16(COLUMNS, TextScreen::COLUMNS as u16)
        .expect("video columns");
    memory
        .write_u16(PAGE_SIZE, PAGE_BYTES as u16)
        .expect("video page size");
    memory.write_u16(PAGE_OFFSET, 0).expect("video page offset");
    memory.write(CURSORS, &[0; 16]).expect("video cursors");
    set_cursor_shape(memory, io, 0x0607);
    select_page(memory, io, 0);
    // BIOS data: color CRTC port, last text row and character height.
    memory
        .write_u16(CRTC_PORT, CRTC_INDEX_PORT)
        .expect("CRTC port");
    let [height_lo, height_hi] = text_mode::CHARACTER_HEIGHT.to_le_bytes();
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
    write_indexed(
        io,
        SEQUENCER_INDEX_PORT,
        sequencer::RESET,
        sequencer::RESET_RELEASED,
    );
}
