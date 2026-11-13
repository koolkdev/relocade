// Serial and interleaved shifters share eight-dot clocks and attribute colors.
fn graphics16_color(input: DisplayVertex, line: Scanline, x: u32) -> u32 {
    let shift_mode = input.display.w;
    let pixel = x + line.pel_pan;
    let fetch = graphics_fetch(input, line, pixel, 8u);
    var color = 0u;
    if (shift_mode & 0x20u) != 0u {
        // Four pairs from maps 0/2, then four from maps 1/3. Maps 2/3
        // supply the upper color bits even in this CGA-compatible shift mode.
        let plane = (pixel & 7u) / 4u;
        let shift = 6u - (pixel & 3u) * 2u;
        let low = (vga_byte(plane, fetch.address) >> shift) & 3u;
        let high = (vga_byte(plane + 2u, fetch.address) >> shift) & 3u;
        color = low | (high << 2u);
    } else {
        // Serial output takes one bit from each plane, high bit first.
        let bit = 7u - (pixel & 7u);
        for (var plane = 0u; plane < 4u; plane++) {
            color |= ((vga_byte(plane, fetch.address) >> bit) & 1u) << plane;
        }
    }
    return palette_color(input, serializer_output(color, fetch.clocks_since_load));
}
