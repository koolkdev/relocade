// CRTC counter pins: byte/word/doubleword rotation and row-scan substitution.
fn display_address(counter: u32, row_scan: u32, address_mode: u32, underline: u32) -> u32 {
    let bounded_counter = counter & state.plane_address_mask;
    var address = bounded_counter;
    if state.linear_addressing != 0u {
        address = bounded_counter;
    } else if (underline & 0x40u) != 0u {
        address = (bounded_counter << 2u) | ((bounded_counter >> 12u) & 3u);
    } else if (address_mode & 0x40u) == 0u {
        let wrap = select(13u, 15u, (address_mode & 0x20u) != 0u);
        address = (bounded_counter << 1u) | ((bounded_counter >> wrap) & 1u);
    }
    if (address_mode & 1u) == 0u {
        address = (address & ~0x2000u) | ((row_scan & 1u) << 13u);
    }
    if (address_mode & 2u) == 0u {
        address = (address & ~0x4000u) | ((row_scan & 2u) << 13u);
    }
    return address & state.plane_address_mask;
}

struct Scanline {
    counter: u32,
    row_scan: u32,
    byte_pan: u32,
    pel_pan: u32,
}

// The row counter, stride and split address apply to text and graphics alike.
fn display_scanline(input: DisplayVertex, y: u32) -> Scanline {
    let scanline_step = input.scanlines.y;
    let physical_row = y * scanline_step;
    let scan_repeat = input.scanlines.x;
    let row_height = input.grid.z;
    let preset_row_scan = input.panning.x;
    let line_compare = input.panning.w;
    let stride = input.addressing.y;
    let attribute_mode = input.display.x;
    var row = physical_row / scan_repeat + preset_row_scan;
    var start = input.addressing.x;
    var byte_pan = input.panning.y;
    var pel_pan = input.panning.z;
    if physical_row > line_compare {
        row = (physical_row - line_compare - 1u) / scan_repeat;
        start = 0u;
        if (attribute_mode & 0x20u) != 0u {
            // Pixel Panning Mode resets both shifts below the line compare.
            byte_pan = 0u;
            pel_pan = 0u;
        }
    }
    return Scanline(
        start + (row / row_height) * stride,
        row % row_height,
        byte_pan,
        pel_pan,
    );
}
