// Physical VRAM and fullscreen drawing are independent of pixel decoding.
@group(0) @binding(1) var<storage, read> memory: array<u32>;

fn packed_byte(packed: u32, index: u32) -> u32 {
    return (packed >> ((index & 3u) * 8u)) & 255u;
}

fn physical_byte(address: u32) -> u32 {
    return packed_byte(memory[address / 4u], address);
}

fn fullscreen_position(index: u32) -> vec4<f32> {
    let positions = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(positions[index], 0.0, 1.0);
}
