#[path = "character_reads/indexed.rs"]
mod indexed;
#[path = "character_reads/packed.rs"]
mod packed;
#[path = "character_reads/planar.rs"]
mod planar;

use crate::video::video;
use relocade_dos::Dos;

const GLYPH: [u8; 8] = [0x81, 0x42, 0x24, 0x18, 0xff, 0, 0x80, 1];

fn read_character(dos: &mut Dos, page: u8) -> u16 {
    let bx = u16::from(page) << 8 | 0x5a;
    let result = video(dos, 0x08a5, bx, 0x1234, 0x5678);
    assert_eq!(result.registers.ebx as u16, bx);
    assert_eq!(result.registers.ecx as u16, 0x1234);
    assert_eq!(result.registers.edx as u16, 0x5678);
    result.registers.eax as u16
}
