// Graphics serializers load independently of the CRTC memory-address counter.
struct GraphicsFetch {
    address: u32,
    clocks_since_load: u32,
}

fn graphics_fetch(input: DisplayVertex, line: Scanline, pixel: u32, pixels_per_clock: u32) -> GraphicsFetch {
    let clocking = sequencer(1u);
    // SR01 Shift 4 takes precedence over Shift Load.
    let load_interval = select(select(1u, 2u, (clocking & 4u) != 0u), 4u, (clocking & 0x10u) != 0u);
    // Assume a fresh load at the unpanned start of each scanline; panning advances
    // within that sequence. The reload phase relative to horizontal blanking
    // has not been verified on original IBM VGA.
    let clock = pixel / pixels_per_clock + line.byte_pan;
    let clocks_since_load = clock % load_interval;
    let load_clock = clock - clocks_since_load;
    let count_divisor = input.grid.w;
    let address_mode = input.addressing.z;
    let underline = input.addressing.w;
    let counter = line.counter + load_clock / count_divisor;
    return GraphicsFetch(
        display_address(counter, line.row_scan, address_mode, underline),
        clocks_since_load,
    );
}

// After each eight shifts, serializer 1 feeds output 0, and so on. Apply this
// to the packed four-bit output, before attribute masking/palette or DAC lookup.
// Circular feedback follows the documented convention in the VGA README.
fn serializer_output(color: u32, clocks_since_load: u32) -> u32 {
    return ((color >> clocks_since_load) | (color << (4u - clocks_since_load))) & 15u;
}
