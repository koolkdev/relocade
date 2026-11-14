// Text and four-bit graphics share sixteen DAC colors resolved at the vertices.
fn palette_color(input: DisplayVertex, index: u32) -> u32 {
    let colors = array(input.colors0, input.colors1, input.colors2, input.colors3);
    return colors[index / 4u][index % 4u];
}

fn attribute_color(color: u32) -> u32 {
    let palette = state.palette[state.attribute_blink_on];
    return dac_color(packed_byte(palette[color / 4u], color));
}
