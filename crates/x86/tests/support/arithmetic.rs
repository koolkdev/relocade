use wasm86_x86::StatusFlags;

use super::machine::Image;

pub(crate) fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.flags.kind = 0;
    // Stale status bits must not win over a new recipe.
    image.cpu.flags.status = StatusFlags {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 1,
        of: 1,
    };
    // DF is unrelated to these binary operations and SETcc; the last byte remains a canary.
    image.cpu.flags.non_status = [0, 1, 0, 0, 0, 0xa5];
    image
}
