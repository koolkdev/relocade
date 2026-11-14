// The 256-color shifter serializes one byte from each physical plane.
// CPU chain-4 controls writes; display addressing remains a CRTC operation.
fn indexed256_color(line: Scanline, x: u32) -> u32 {
    let pixel = x + line.pel_pan;
    let fetch = graphics_fetch(line, pixel);
    let color = vga_byte(pixel & 3u, fetch.address);
    // The Attribute Controller combines two successive four-bit outputs.
    let high = serializer_output(color >> 4u, fetch.clocks_since_load);
    let low = serializer_output(color & 15u, fetch.clocks_since_load);
    return dac_color((high << 4u) | low);
}
