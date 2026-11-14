// Apply prepared CRTC pin routing to the changing address and row counters.
fn display_address(counter: u32, row_scan: u32) -> u32 {
    let bounded_counter = counter & state.plane_address_mask;
    let address = (bounded_counter << state.address_shift)
        | ((bounded_counter >> state.wrap_shift) & state.wrap_mask);
    let substituted = (address & ~state.row_scan_mask) | ((row_scan << 13u) & state.row_scan_mask);
    return substituted & state.plane_address_mask;
}

struct Scanline {
    counter: u32,
    row_scan: u32,
    byte_pan: u32,
    pel_pan: u32,
}

// The row counter, stride and split address apply to text and graphics alike.
fn display_scanline(y: u32) -> Scanline {
    let physical_row = y * state.scanline_step;
    var row = physical_row / state.scan_repeat + state.preset_row;
    var start = state.display_start;
    var byte_pan = state.byte_pan;
    var pel_pan = state.pel_pan;
    if physical_row >= state.split_scanline {
        row = (physical_row - state.split_scanline) / state.scan_repeat;
        start = 0u;
        if state.reset_panning != 0u {
            byte_pan = 0u;
            pel_pan = 0u;
        }
    }
    return Scanline(
        start + (row / state.row_height) * state.stride,
        row % state.row_height,
        byte_pan,
        pel_pan,
    );
}
