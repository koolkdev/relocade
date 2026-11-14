// Blanking boundaries are prepared in output pixel coordinates.
fn in_blanking(count: u32, intervals: vec4<u32>) -> bool {
    return (count >= intervals.x && count < intervals.y)
        || (count >= intervals.z && count < intervals.w);
}

fn pixel_blanked(pixel: vec2<u32>) -> bool {
    return in_blanking(pixel.x, state.horizontal_blank)
        || in_blanking(pixel.y, state.vertical_blank);
}
