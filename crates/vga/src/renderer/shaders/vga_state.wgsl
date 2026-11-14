// VGA settings occupy 160 bytes, including final structure padding. Raw banks
// contain four bytes per word; offsets match renderer/state.rs.
struct VgaState {
    sequencer: vec4<u32>,
    crtc: array<vec4<u32>, 2>,
    graphics: vec4<u32>,
    attributes: array<vec4<u32>, 2>,
    miscellaneous: u32,
    display_enabled: u32,
    attribute_blink_on: u32,
    cursor_blink_on: u32,
    scanline_step: u32,
    display_start: u32,
    linear_addressing: u32,
    plane_address_mask: u32,
    cursor_address: u32,
    line_compare: u32,
    vertical_blanking: vec4<u32>,
}

@group(0) @binding(0) var<uniform> state: VgaState;

fn sequencer(index: u32) -> u32 {
    return packed_byte(state.sequencer[index / 4u], index);
}

fn crtc(index: u32) -> u32 {
    return packed_byte(state.crtc[index / 16u][(index / 4u) % 4u], index);
}

fn graphics(index: u32) -> u32 {
    return packed_byte(state.graphics[index / 4u], index);
}

fn attribute_reg(index: u32) -> u32 {
    return packed_byte(state.attributes[index / 16u][(index / 4u) % 4u], index);
}

fn vga_byte(plane: u32, address: u32) -> u32 {
    // Each word contains the four planes at one VGA plane offset.
    return packed_byte(memory[address & state.plane_address_mask], plane);
}
