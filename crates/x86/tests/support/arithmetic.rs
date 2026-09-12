use wasm86_x86::FlagBytes;

use super::machine::Image;

pub(crate) fn image(code: &[u8]) -> Image {
    let mut image = Image::new(code);
    image.cpu.flags.status_source.kind = 0;
    // Stale status bits must not win over a new recipe.
    image.cpu.flags.bytes = FlagBytes {
        cf: 1,
        pf: 1,
        af: 1,
        zf: 1,
        sf: 1,
        of: 1,
        ..image.cpu.flags.bytes
    };
    // DF is unrelated to these binary operations and SETcc; the last byte remains a canary.
    image.cpu.flags.bytes = FlagBytes {
        tf: 0,
        df: 1,
        nt: 0,
        ac: 0,
        id: 0,
        reserved: 0xa5,
        ..image.cpu.flags.bytes
    };
    image
}
