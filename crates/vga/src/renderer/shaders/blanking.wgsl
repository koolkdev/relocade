// CRTC blanking uses raster coordinates, independently of memory addressing.
// Boundaries follow the original-VGA convention documented in README.md:
// one-count output delay, six horizontal end bits and seven vertical end bits.

// Return a tail interval [x,y) and a head interval [0,z). The head accounts
// for the counter resetting before the end comparator matches. Equal start/end
// gives set priority; an unreachable end stays blank in steady state.
fn blanking_intervals(start: u32, end_low: u32, mask: u32, total: u32) -> vec3<u32> {
    if start >= total {
        return vec3(0u);
    }
    let end = start + (((end_low - start - 1u) & mask) + 1u) + 1u;
    let head_end = select(0u, min(end_low + 1u, total), end > total);
    if head_end > start {
        return vec3(0u, total, 0u);
    }
    return vec3(start + 1u, min(end, total), head_end);
}

fn horizontal_blanking() -> vec3<u32> {
    let end_low = (crtc(3u) & 31u) | ((crtc(5u) & 0x80u) >> 2u);
    let intervals = blanking_intervals(crtc(2u), end_low, 63u, crtc(0u) + 5u);
    // The cropped image starts at display enable, including its character skew.
    let skew = vec3((crtc(3u) >> 5u) & 3u);
    return intervals - min(intervals, skew);
}

fn vertical_blanking() -> vec3<u32> {
    let overflow = crtc(7u);
    let start = crtc(0x15u) | ((overflow & 8u) << 5u) | ((crtc(9u) & 0x20u) << 4u);
    let total = crtc(6u) | ((overflow & 1u) << 8u) | ((overflow & 0x20u) << 4u);
    return blanking_intervals(start, crtc(0x16u) & 127u, 127u, total + 2u);
}

fn in_blanking(count: u32, intervals: vec3<u32>) -> bool {
    return count < intervals.z || (count >= intervals.x && count < intervals.y);
}

fn pixel_blanked(input: DisplayVertex, pixel: vec2<u32>) -> bool {
    let indexed256 = (input.display.x & 0x41u) == 0x41u && (input.display.w & 0x40u) != 0u;
    let horizontal_count = pixel.x * select(1u, 2u, indexed256) / input.grid.y;
    let vertical_divisor = 1u << ((input.addressing.z >> 2u) & 1u);
    let vertical_count = pixel.y * input.scanlines.y / vertical_divisor;
    return in_blanking(horizontal_count, input.horizontal_blank)
        || in_blanking(vertical_count, input.vertical_blank);
}
