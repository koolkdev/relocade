//! BIOS DAC defaults. Attribute palette indices remain part of each mode preset.
use relocade_vga::registers::text_mode;

// Sixteen PC colors, sixteen grays, then nine 24-hue rings.
// Entries 248–255 are black.
pub(super) const INDEXED256: [[u8; 3]; 256] = {
    let mut colors = [[0; 3]; 256];
    let mut index = 0;
    while index < 16 {
        colors[index] = text_mode::DAC_PALETTE[text_mode::ATTRIBUTE_PALETTE[index] as usize];
        index += 1;
    }
    let grays = [0, 5, 8, 11, 14, 17, 20, 24, 28, 32, 36, 40, 45, 50, 56, 63];
    index = 0;
    while index < 16 {
        colors[16 + index] = [grays[index]; 3];
        index += 1;
    }
    let levels = [
        [0, 16, 31, 47, 63],
        [31, 39, 47, 55, 63],
        [45, 49, 54, 58, 63],
        [0, 7, 14, 21, 28],
        [14, 17, 21, 24, 28],
        [20, 22, 24, 26, 28],
        [0, 4, 8, 12, 16],
        [8, 10, 12, 14, 16],
        [11, 12, 13, 15, 16],
    ];
    let mut ring = 0;
    while ring < levels.len() {
        let ramp = levels[ring];
        let low = ramp[0];
        let high = ramp[4];
        let mut step = 0;
        while step < 4 {
            let up = ramp[step];
            let down = ramp[4 - step];
            let start = 32 + ring * 24;
            colors[start + step] = [up, low, high]; // Blue to magenta.
            colors[start + 4 + step] = [high, low, down];
            colors[start + 8 + step] = [high, up, low];
            colors[start + 12 + step] = [down, high, low];
            colors[start + 16 + step] = [low, high, up];
            colors[start + 20 + step] = [low, down, high];
            step += 1;
        }
        ring += 1;
    }
    colors
};

// The 200-line, 16-color modes use RGBI compatibility: bit 4 is intensity,
// bits 3 and 5 are ignored. This differs from the 64-color EGA DAC table.
pub(super) const RGBI: [[u8; 3]; 256] = {
    let mut colors = [[0; 3]; 256];
    let mut index = 0;
    while index < 64 {
        let pc_color = (index & 7) | ((index & 0x10) >> 1);
        colors[index] = INDEXED256[pc_color];
        index += 1;
    }
    colors
};
