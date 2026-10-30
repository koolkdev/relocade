// CRTC counter pins: byte/word/doubleword rotation and row-scan substitution.
fn display_address(counter: u32, row: u32, mode: u32, underline: u32) -> u32 {
    var address = counter;
    if (underline & 0x40u) != 0u {
        address = (counter << 2u) | ((counter >> 12u) & 3u);
    } else if (mode & 0x40u) == 0u {
        let wrap = select(13u, 15u, (mode & 0x20u) != 0u);
        address = (counter << 1u) | ((counter >> wrap) & 1u);
    }
    if (mode & 1u) == 0u {
        address = (address & ~0x2000u) | ((row & 1u) << 13u);
    }
    if (mode & 2u) == 0u {
        address = (address & ~0x4000u) | ((row & 2u) << 13u);
    }
    return address & 0xffffu;
}

struct Scanline {
    counter: u32,
    row_scan: u32,
    pel_pan: u32,
}

// The row counter, stride and split address apply to text and graphics alike.
fn display_scanline(input: DisplayVertex, y: u32) -> Scanline {
    let physical_row = y * input.scanlines.y;
    let scan_repeat = input.scanlines.x;
    let row_height = input.grid.z;
    let line_compare = input.panning.w;
    var row = physical_row / scan_repeat + input.panning.x;
    var start = input.addressing.x;
    var pan = input.panning.z;
    if physical_row > line_compare {
        row = (physical_row - line_compare - 1u) / scan_repeat;
        start = 0u;
        if (input.display.x & 0x20u) != 0u {
            pan = 0u;
        }
    }
    return Scanline(
        start + (row / row_height) * input.addressing.y + input.panning.y,
        row % row_height,
        pan,
    );
}
