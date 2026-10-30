// The serial shifter takes one bit from each plane, most-significant bit first.
fn planar16_color(input: DisplayVertex, line: Scanline, x: u32) -> u32 {
    let pixel = x + line.pel_pan;
    let counter = (line.counter + (pixel / 8u) / input.grid.w) & 0xffffu;
    let address = display_address(counter, line.row_scan, input.addressing.z, input.addressing.w);
    let bit = 7u - (pixel & 7u);
    var color = 0u;
    for (var plane = 0u; plane < 4u; plane++) {
        color |= ((vram_byte(plane, address) >> bit) & 1u) << plane;
    }
    return palette_color(input, color);
}
