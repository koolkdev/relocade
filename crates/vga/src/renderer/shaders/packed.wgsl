// Packed settings are independent of legacy VGA registers and vertex outputs.
struct PackedState {
    pitch: u32,
    display_start: u32,
}

@group(0) @binding(0) var<uniform> state: PackedState;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return fullscreen_position(index);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(position.xy);
    let address = state.display_start + pixel.y * state.pitch + pixel.x;
    // Each 256-KiB group interleaves four bytes across four 64-KiB blocks.
    let block = (address / 262144u) * 4u + (address & 3u);
    return unpack4x8unorm(dac_color(vram_byte(block, address / 4u)));
}
