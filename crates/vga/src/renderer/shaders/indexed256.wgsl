// The 256-color shifter serializes one byte from each physical plane.
// CPU chain-4 controls writes; display addressing remains a CRTC operation.
fn indexed256_color(input: DisplayVertex, line: Scanline, x: u32) -> u32 {
    let pixel = x + line.pel_pan;
    let counter = (line.counter + (pixel / 4u) / input.grid.w) & 0xffffu;
    let address = display_address(counter, line.row_scan, input.addressing.z, input.addressing.w);
    return dac_color(vram_byte(pixel & 3u, address));
}
