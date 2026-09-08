use super::machine::Image;

pub(super) fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu[0] = 0;
    image.cpu[12..23].fill(0);
    image.cpu[12..18].fill(1); // Stale status bits must not win over a new recipe.
    image.cpu[19] = 1; // DF is unrelated to ADD, CMP and SETcc.
    image
}

// These fields assert the external lazy-record ABI, not architectural flag bits.
// Padding and concrete flag bytes retain the fixture's original contents.
pub(super) fn recipe(kind: u8, left: u32, right: u32) -> [(usize, u32); 3] {
    [(0, 0xa5a5_a500 | u32::from(kind)), (4, left), (8, right)]
}
