// CRTC counter pins: byte/word/doubleword rotation and row-scan substitution.
fn display_address(counter: u32, row: u32, mode: u32, underline: u32) -> u32 {
    var address = counter;
    if (underline & 0x40u) != 0u {
        address = (counter << 2u) | ((counter >> 12u) & 3u);
    } else if (mode & 0x40u) == 0u {
        let wrap = select(13u, 15u, (mode & 0x20u) != 0u);
        address = (counter << 1u) | ((counter >> wrap) & 1u);
    }
    if (mode & 1u) == 0u {
        address = (address & ~0x2000u) | ((row & 1u) << 13u);
    }
    if (mode & 2u) == 0u {
        address = (address & ~0x4000u) | ((row & 2u) << 13u);
    }
    return address & 0xffffu;
}
