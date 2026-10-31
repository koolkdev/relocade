// Captured input bindings. Raw register banks contain four bytes per word;
// the uniform layout matches GpuState in renderer/state.rs.
struct State {
    sequencer: vec4<u32>,
    crtc: array<vec4<u32>, 2>,
    graphics: vec4<u32>,
    attributes: array<vec4<u32>, 2>,
    miscellaneous: u32,
    attribute_address: u32,
    dac_mask: u32,
    blink_on: u32,
    scanline_step: u32,
    display_start: u32,
    reserved1: u32,
    reserved2: u32,
    palette: array<vec4<u32>, 48>,
}

@group(0) @binding(0) var<uniform> state: State;
@group(0) @binding(1) var<storage, read> memory: array<u32>;

struct UpdateBounds {
    starts: vec4<u32>,
    ends: vec4<u32>,
}
@group(1) @binding(0) var<uniform> updates: UpdateBounds;

// Decoders discard and return on unchanged fetches: discard suppresses pixel
// writes, while the explicit return avoids the remaining decoding calculations.
fn vram_changed(plane: u32, address: u32) -> bool {
    return address >= updates.starts[plane] && address < updates.ends[plane];
}

fn packed_byte(packed: u32, index: u32) -> u32 {
    return (packed >> ((index & 3u) * 8u)) & 255u;
}

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

fn dac_component(index: u32) -> u32 {
    return packed_byte(state.palette[index / 16u][(index / 4u) % 4u], index);
}

fn vram_byte(plane: u32, address: u32) -> u32 {
    return packed_byte(memory[plane * 16384u + (address & 0xffffu) / 4u], address);
}
