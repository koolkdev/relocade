// VGA and packed SVGA share the captured DAC width, mask and RGB palette.
struct DacState {
    mask: u32,
    component_mask: u32,
    // vec4 alignment leaves eight padding bytes after the controls.
    palette: array<vec4<u32>, 48>,
}

@group(0) @binding(2) var<uniform> dac: DacState;

fn dac_component(index: u32) -> u32 {
    return packed_byte(dac.palette[index / 16u][(index / 4u) % 4u], index);
}

// Apply the DAC mask and normalize the captured precision to eight-bit RGBA.
fn dac_color(index: u32) -> u32 {
    let offset = (index & dac.mask) * 3u;
    let rgb = vec3(dac_component(offset), dac_component(offset + 1u), dac_component(offset + 2u));
    let rgb8 = (rgb * 255u + vec3(dac.component_mask / 2u)) / vec3(dac.component_mask);
    return rgb8.r | (rgb8.g << 8u) | (rgb8.b << 16u) | 0xff000000u;
}
