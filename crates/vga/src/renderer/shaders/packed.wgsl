// Packed settings are independent of legacy VGA registers and vertex outputs.
struct PackedState {
    pitch: u32,
    display_start: u32,
    format: u32,
}

@group(0) @binding(0) var<uniform> state: PackedState;

// Decoder tags shared with renderer/state.rs.
const INDEXED8 = 0u;
const RGB565 = 1u;
const BGRX8888 = 2u;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    return fullscreen_position(index);
}

@fragment
fn fragment(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<u32>(position.xy);
    let row = state.display_start + pixel.y * state.pitch;
    switch state.format {
        case INDEXED8: {
            return unpack4x8unorm(dac_color(physical_byte(row + pixel.x)));
        }
        case RGB565: {
            let address = row + pixel.x * 2u;
            let value = physical_byte(address) | (physical_byte(address + 1u) << 8u);
            let rgb = vec3(value >> 11u, (value >> 5u) & 63u, value & 31u);
            let rgb8 = (rgb * 255u + vec3(15u, 31u, 15u)) / vec3(31u, 63u, 31u);
            return vec4(vec3<f32>(rgb8) / 255.0, 1.0);
        }
        case BGRX8888: {
            let address = row + pixel.x * 4u;
            let rgb = vec3(physical_byte(address + 2u), physical_byte(address + 1u), physical_byte(address));
            return vec4(vec3<f32>(rgb) / 255.0, 1.0);
        }
        default: { return vec4(0.0, 0.0, 0.0, 1.0); }
    }
}
