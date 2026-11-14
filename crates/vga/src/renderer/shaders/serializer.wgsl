// Graphics serializers load independently of the CRTC memory-address counter.
struct GraphicsFetch {
    address: u32,
    clocks_since_load: u32,
}

fn graphics_fetch(line: Scanline, pixel: u32) -> GraphicsFetch {
    // Assume a fresh load at the unpanned start of each scanline; panning advances
    // within that sequence. The reload phase relative to horizontal blanking
    // has not been verified on original IBM VGA.
    let clock = pixel / state.pixels_per_clock + line.byte_pan;
    let clocks_since_load = clock % state.load_interval;
    let load_clock = clock - clocks_since_load;
    let counter = line.counter + load_clock / state.count_divisor;
    return GraphicsFetch(
        display_address(counter, line.row_scan),
        clocks_since_load,
    );
}

// After each eight shifts, serializer 1 feeds output 0, and so on. Apply this
// to the packed four-bit output, before attribute masking/palette or DAC lookup.
// Circular feedback follows the documented convention in the VGA README.
fn serializer_output(color: u32, clocks_since_load: u32) -> u32 {
    return ((color >> clocks_since_load) | (color << (4u - clocks_since_load))) & 15u;
}
