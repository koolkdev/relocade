/// One text-memory cell. Attribute bits encode foreground, background and blink.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextCell {
    pub character: u8,
    pub attribute: u8,
}

/// Hardware cursor registers. The address names a cell in video memory.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TextCursor {
    pub address: u16,
    /// Low five bits select the first scan line; bit 5 disables the cursor.
    pub start_scan_line: u8,
    /// Low five bits select the last scan line; bits 5–6 specify cursor skew.
    pub end_scan_line: u8,
}

/// An owned 80-by-25 text-memory window for BIOS inspection and debugging.
/// Programmable display geometry and font output are carried by pixel frames.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextScreen {
    pub cells: [TextCell; Self::COLUMNS * Self::ROWS],
    pub cursor: TextCursor,
    /// CRTC start address in cells, including starts within a BIOS text page.
    pub start_address: u16,
}

impl TextScreen {
    pub const COLUMNS: usize = 80;
    pub const ROWS: usize = 25;
}
