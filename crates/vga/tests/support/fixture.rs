// Prepare font memory for VGA fixtures that construct a device without BIOS.
#![allow(dead_code)]
use relocade_machine::Device;
use relocade_vga::registers::text_mode;

pub fn initialize_font(vga: &mut impl Device) {
    for (port, value) in [
        (0x3c4, 0x0402),
        (0x3c4, 0x0604),
        (0x3ce, 0x0005),
        (0x3ce, 0x0506),
    ] {
        vga.write_port(port, 2, value);
    }
    for (character, glyph) in text_mode::FONT.as_chunks::<8>().0.iter().enumerate() {
        for (row, byte) in glyph.iter().enumerate() {
            vga.write_mmio(
                0xa0000 + character as u32 * 32 + row as u32,
                1,
                u64::from(*byte),
            );
        }
    }
    for (port, value) in [
        (0x3c4, 0x0302),
        (0x3c4, 0x0204),
        (0x3ce, 0x1005),
        (0x3ce, 0x0e06),
    ] {
        vga.write_port(port, 2, value);
    }
}

pub struct StaticClock;
impl relocade_machine::Clock for StaticClock {
    fn now(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }
    async fn wait_until(&self, _: std::time::Duration) {
        std::future::pending::<()>().await;
    }
}

pub fn text_device() -> relocade_vga::Vga<StaticClock> {
    let mut vga = relocade_vga::Vga::with_clock(std::rc::Rc::new(StaticClock));
    initialize_font(&mut vga);
    for cell in 0..2000 {
        set_cell(&mut vga, cell, b' ', 7);
    }
    vga.write_port(0x3d4, 2, 0x200a);
    vga
}

pub fn set_cell(vga: &mut impl Device, cell: usize, character: u8, attribute: u8) {
    vga.write_mmio(
        0xb8000 + cell as u32 * 2,
        2,
        u64::from(u16::from_le_bytes([character, attribute])),
    );
}

pub fn capture<C: relocade_machine::Clock>(
    vga: &relocade_vga::Vga<C>,
) -> relocade_vga::CapturedFrame {
    relocade_vga::CapturedFrame {
        display: vga.snapshot(),
        time: std::time::Duration::from_millis(123),
        sequence: 42,
    }
}
