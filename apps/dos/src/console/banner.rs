//! CP437 block lettering with ANSI foreground colors.

const LOGO: [&str; 6] = [
    "██████╗ ███████╗██╗      ██████╗  ██████╗ █████╗ ██████╗ ███████╗",
    "██╔══██╗██╔════╝██║     ██╔═══██╗██╔════╝██╔══██╗██╔══██╗██╔════╝",
    "██████╔╝█████╗  ██║     ██║   ██║██║     ███████║██║  ██║█████╗",
    "██╔══██╗██╔══╝  ██║     ██║   ██║██║     ██╔══██║██║  ██║██╔══╝",
    "██║  ██║███████╗███████╗╚██████╔╝╚██████╗██║  ██║██████╔╝███████╗",
    "╚═╝  ╚═╝╚══════╝╚══════╝ ╚═════╝  ╚═════╝╚═╝  ╚═╝╚═════╝ ╚══════╝",
];

// Each row has a fill and shadow: white, bright cyan, cyan, then blue.
const COLORS: [(&[u8], &[u8]); 6] = [
    (b"\x1b[1;37m", b"\x1b[1;34m"),
    (b"\x1b[1;36m", b"\x1b[1;34m"),
    (b"\x1b[1;36m", b"\x1b[1;34m"),
    (b"\x1b[0;36m", b"\x1b[0;34m"),
    (b"\x1b[1;34m", b"\x1b[0;34m"),
    (b"\x1b[0;34m", b"\x1b[0;34m"),
];

pub(super) fn bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    for (line, (fill, shadow)) in LOGO.into_iter().zip(COLORS) {
        bytes.extend_from_slice(b"   ");
        let mut current_color: &[u8] = &[];
        for character in line.chars() {
            let color = if character == '█' { fill } else { shadow };
            if character != ' ' && color != current_color {
                bytes.extend_from_slice(color);
                current_color = color;
            }
            bytes.push(glyph(character));
        }
        bytes.extend_from_slice(b"\r\n");
    }
    bytes.extend_from_slice(b"\x1b[0m\r\n");
    bytes
}

// Keep the artwork readable above; DOS receives CP437 bytes, not UTF-8.
fn glyph(character: char) -> u8 {
    match character {
        ' ' => b' ',
        '█' => 0xdb,
        '╗' => 0xbb,
        '╔' => 0xc9,
        '═' => 0xcd,
        '╝' => 0xbc,
        '║' => 0xba,
        '╚' => 0xc8,
        _ => unreachable!("banner glyph"),
    }
}
