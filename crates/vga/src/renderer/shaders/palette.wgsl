// Text and planar graphics share the sixteen colors decoded at each vertex.
fn palette_color(input: DisplayVertex, index: u32) -> u32 {
    let colors = array(input.colors0, input.colors1, input.colors2, input.colors3);
    return colors[index / 4u][index % 4u];
}

// The attribute controller maps four-bit colors to entries in the DAC palette.
fn attribute_color(color: u32) -> u32 {
    let select_bits = attribute_reg(0x14u);
    var index = attribute_reg(color & attribute_reg(0x12u));
    if (attribute_reg(0x10u) & 0x80u) != 0u {
        index = (index & 15u) | ((select_bits & 3u) << 4u);
    }
    return dac_color(index | ((select_bits & 12u) << 4u));
}

// Apply the DAC mask and expand six-bit RGB to packed eight-bit RGBA.
fn dac_color(index: u32) -> u32 {
    let offset = (index & state.dac_mask) * 3u;
    let rgb = vec3(dac_component(offset), dac_component(offset + 1u), dac_component(offset + 2u));
    let rgb8 = (rgb * 255u + vec3(31u)) / vec3(63u);
    return rgb8.r | (rgb8.g << 8u) | (rgb8.b << 16u) | 0xff000000u;
}
