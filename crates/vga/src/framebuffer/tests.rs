use super::*;
use crate::PackedLayout;

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn bgr888_accesses_three_bytes_across_blocks_and_wrapping_vram() {
    for wrapping in [false, true] {
        let mut memory = VideoMemory::new(if wrapping { 65536 } else { 131072 });
        let layout = if wrapping {
            PackedLayout::wrapping([2, 2], PixelFormat::Bgr888, 15, 65535, 65536)
        } else {
            PackedLayout::new([2, 2], PixelFormat::Bgr888, 15, 65535, 131072)
        }
        .unwrap();
        let mask = if wrapping { 0xffff } else { u32::MAX };
        assert_eq!(layout.format().bits_per_pixel(), 24);
        assert_eq!(layout.format().bytes_per_pixel(), 3);
        for (address, value) in [
            (65534, 0xa5),
            (65535, 0x11),
            (65536, 0x44),
            (65537, 0x33),
            (65538, 0xa5),
        ] {
            memory.write_physical(address & mask, value);
        }
        let captured = memory.snapshot(memory.capacity(), None);
        let mut frame = Framebuffer {
            memory: &mut memory,
            scanout: Scanout::Packed(layout),
        };
        let pixel = frame.pixel(0, 0).unwrap();
        assert_eq!(pixel.value(), 0x334411);
        assert_eq!(pixel.color_mask(), 0xffffff);
        frame.write_pixel(&pixel, 0xff000000 | (pixel.value() ^ pixel.color_mask()));
        assert_eq!(frame.pixel(0, 0).unwrap().value(), 0xccbbee);
        assert_eq!(frame.memory.read_physical(65534), 0xa5);
        assert_eq!(frame.memory.read_physical(65538 & mask), 0xa5);
        let next_row = frame.pixel(1, 1).unwrap();
        frame.write_pixel(&next_row, 0x123456);
        for (address, value) in [(65553, 0x56), (65554, 0x34), (65555, 0x12)] {
            assert_eq!(frame.memory.read_physical(address & mask), value);
        }
        // Restoring a sampled pixel uses its original address and wrap policy.
        frame.scanout = Scanout::Packed(layout.with_origin([0, 0]).unwrap());
        frame.write_pixel(&pixel, pixel.value());
        for (address, value) in [(65535, 0x11), (65536, 0x44), (65537, 0x33)] {
            assert_eq!(frame.memory.read_physical(address & mask), value);
            assert_eq!(captured.read(address & mask), Some(value));
        }
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn rgb555_uses_two_physical_bytes_and_preserves_the_spare_bit_when_inverting() {
    let mut memory = VideoMemory::new(2 * 65536);
    let layout =
        PackedLayout::new([2, 2], PixelFormat::Rgb555, 65536, 65532, memory.capacity()).unwrap();
    assert_eq!(layout.format().bits_per_pixel(), 15);
    assert_eq!(layout.format().bytes_per_pixel(), 2);
    assert!(PackedLayout::new([2, 2], PixelFormat::Rgb555, 5, 0, memory.capacity()).is_none());
    memory.write_physical(65534, 0x1f);
    memory.write_physical(65535, 0x80);
    memory.write_physical(65536, 0x5a);
    let captured = memory.snapshot(memory.capacity(), None);
    let mut frame = Framebuffer {
        memory: &mut memory,
        scanout: Scanout::Packed(layout),
    };
    let pixel = frame.pixel(1, 0).unwrap();
    assert_eq!(pixel.value(), 0x801f);
    assert_eq!(pixel.color_mask(), 0x7fff);
    frame.write_pixel(&pixel, pixel.value() ^ pixel.color_mask());
    assert_eq!(frame.pixel(1, 0).unwrap().value(), 0xffe0);
    let next_row = frame.pixel(0, 1).unwrap();
    frame.write_pixel(&next_row, 0x7c00);
    assert_eq!(frame.memory.read_physical(131068), 0);
    assert_eq!(frame.memory.read_physical(131069), 0x7c);
    // A retained sample restores its physical location after the viewport moves.
    frame.scanout = Scanout::Packed(layout.with_origin([0, 0]).unwrap());
    frame.write_pixel(&pixel, pixel.value());
    assert_eq!(frame.memory.read_physical(65534), 0x1f);
    assert_eq!(frame.memory.read_physical(65535), 0x80);
    assert_eq!(frame.memory.read_physical(65536), 0x5a);
    assert_eq!(captured.read(65534), Some(0x1f));
    assert_eq!(captured.read(65535), Some(0x80));
}
