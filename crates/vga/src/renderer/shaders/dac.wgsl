// VGA and packed SVGA share the DAC mask and six-bit RGB palette.
struct DacState {
    mask: u32,
    // vec4 alignment leaves twelve padding bytes after the mask.
    palette: array<vec4<u32>, 48>,
}

@group(0) @binding(2) var<uniform> dac: DacState;

fn dac_component(index: u32) -> u32 {
    return packed_byte(dac.palette[index / 16u][(index / 4u) % 4u], index);
}

// Apply the DAC mask and expand six-bit RGB to packed eight-bit RGBA.
fn dac_color(index: u32) -> u32 {
    let offset = (index & dac.mask) * 3u;
    let rgb = vec3(dac_component(offset), dac_component(offset + 1u), dac_component(offset + 2u));
    let rgb8 = (rgb * 255u + vec3(31u)) / vec3(63u);
    return rgb8.r | (rgb8.g << 8u) | (rgb8.b << 16u) | 0xff000000u;
}
