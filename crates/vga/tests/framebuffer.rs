mod support;

use relocade_clock_test_support::ManualClock;
use relocade_machine::Device;
use relocade_vga::{FramebufferAccess, FramebufferGeometry, PixelFormat, Vga};
use support::{indexed256, planar16, registers::indexed, svga};

#[cfg(target_arch = "wasm32")]
wasm_bindgen_test::wasm_bindgen_test_configure!(run_in_browser);

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn planar_drawing_preserves_cpu_access_state_neighbors_and_retained_captures() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let mut vga = Vga::with_clock(ManualClock::new().clock());
        planar16::program(&mut vga);
        let address = 2 * 80 + 1;
        let originals = [0x5a, 0x6c, 0xa5, 0xc3];
        for (plane, byte) in originals.into_iter().enumerate() {
            indexed(&mut vga, 0x3c4, 2, 1 << plane);
            vga.write_mmio(context.reborrow(), 0xa0000 + address, 1, byte.into());
        }
        vga.read_mmio(context.reborrow(), 0xa0000 + address, 1); // Retain nonzero read latches.
        for (port, index, value) in [
            (0x3c4, 2, 0),
            (0x3ce, 1, 15),
            (0x3ce, 3, 0x1b),
            (0x3ce, 4, 3),
            (0x3ce, 5, 0x0b),
            (0x3ce, 8, 0x27),
        ] {
            indexed(&mut vga, port, index, value);
        }
        let core = vga.core_state();
        let before = vga.snapshot();
        let pixel = {
            let mut frame = vga.framebuffer();
            assert_eq!(
                frame.geometry(),
                Some(FramebufferGeometry::Graphics {
                    width: 640,
                    height: 480
                })
            );
            let pixel = frame.pixel(9, 2).unwrap();
            assert_eq!(
                (pixel.value(), pixel.color_mask(), pixel.format()),
                (11, 15, PixelFormat::Indexed8)
            );
            frame.write_pixel(&pixel, 5);
            assert!(frame.pixel(640, 2).is_none());
            assert!(frame.text_cell(0, 0).is_none());
            pixel
        };
        assert_eq!(vga.core_state(), core);
        for (plane, expected) in [0x5a, 0x2c, 0xe5, 0x83].into_iter().enumerate() {
            assert_eq!(vga.snapshot().plane(plane)[address as usize], expected);
            assert_eq!(before.plane(plane)[address as usize], originals[plane]);
        }
        vga.framebuffer().write_pixel(&pixel, pixel.value());
        for (plane, expected) in originals.into_iter().enumerate() {
            assert_eq!(vga.snapshot().plane(plane)[address as usize], expected);
        }
        assert_eq!(vga.core_state(), core);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode13_writes_the_cpu_framebuffer_bytes_above_the_first_plane_quarter() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let mut vga = Vga::with_clock(ManualClock::new().clock());
        indexed256::program(&mut vga);
        vga.write_mmio(context.reborrow(), 0xa0000 + 199 * 320 + 319, 1, 0xa5);
        let pixel = vga.framebuffer().pixel(319, 199).unwrap();
        assert_eq!(
            (pixel.value(), pixel.format()),
            (0xa5, PixelFormat::Indexed8)
        );
        vga.framebuffer().write_pixel(&pixel, 0x96);
        assert_eq!(
            vga.read_mmio(context.reborrow(), 0xa0000 + 199 * 320 + 319, 1),
            0x96
        );
        vga.framebuffer().write_pixel(&pixel, pixel.value());
        assert_eq!(
            vga.read_mmio(context.reborrow(), 0xa0000 + 199 * 320 + 319, 1),
            0xa5
        );
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn mode_x_uses_programmed_geometry_stride_and_page_but_keeps_restore_locations() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let mut vga = Vga::with_clock(ManualClock::new().clock());
        indexed256::program(&mut vga);
        indexed256::unchain(&mut vga);
        for (register, value) in [
            (6, 0x0b),
            (7, 0x3e),
            (0x12, 0xdf),
            (0x15, 0xe7),
            (0x16, 4),
            (0x13, 48), // 96 plane bytes per row.
            (12, 0x40),
        ] {
            indexed(&mut vga, 0x3d4, register, value);
        }
        indexed(&mut vga, 0x3c4, 2, 8);
        vga.write_mmio(context.reborrow(), 0xa0000 + 0x4000 + 200 * 96 + 1, 1, 0x56);
        let pixel = {
            let mut frame = vga.framebuffer();
            assert_eq!(
                frame.geometry(),
                Some(FramebufferGeometry::Graphics {
                    width: 320,
                    height: 240
                })
            );
            let pixel = frame.pixel(7, 200).unwrap();
            assert_eq!(pixel.value(), 0x56);
            frame.write_pixel(&pixel, 0xa9);
            pixel
        };
        assert_eq!(vga.snapshot().vga().unwrap().display_start, 0);
        assert_eq!(vga.snapshot().plane(3)[0x4000 + 200 * 96 + 1], 0xa9);
        indexed(&mut vga, 0x3d4, 12, 0x50);
        vga.framebuffer().write_pixel(&pixel, pixel.value());
        assert_eq!(vga.snapshot().plane(3)[0x4000 + 200 * 96 + 1], 0x56);
        assert_eq!(vga.snapshot().plane(3)[0x5000 + 200 * 96 + 1], 0);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn text_cells_use_the_programmed_page_without_loading_read_latches() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let mut vga = support::fixture::text_device();
        support::fixture::set_cell(&mut vga, 2048 + 82, b'Q', 0x9a);
        indexed(&mut vga, 0x3d4, 12, 8);
        let core = vga.core_state();
        let cell = {
            let mut frame = vga.framebuffer();
            let cell = frame.text_cell(1, 2).unwrap();
            assert_eq!(cell.value(), 0x9a51);
            frame.write_text_cell(&cell, cell.value() ^ 0x7700);
            cell
        };
        assert_eq!(vga.core_state(), core);
        assert_eq!(
            vga.read_mmio(context.reborrow(), 0xb8000 + (2048 + 82) * 2, 2),
            0xed51
        );
        indexed(&mut vga, 0x3d4, 12, 0);
        vga.framebuffer().write_text_cell(&cell, cell.value());
        assert_eq!(
            vga.read_mmio(context.reborrow(), 0xb8000 + (2048 + 82) * 2, 2),
            0x9a51
        );
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn svga_pixels_use_virtual_stride_and_origin_without_changing_cpu_bank_or_state() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        for (bits, value, mask, format) in [
            (8, 0xa5, 255, PixelFormat::Indexed8),
            (16, 0xa55a, 0xffff, PixelFormat::Rgb565),
            (32, 0xa5123456, 0xffffff, PixelFormat::Bgrx8888),
        ] {
            let mut vga = Vga::with_clock(ManualClock::new().clock());
            svga::set_mode(&mut vga, 640, 480, bits);
            svga::set(&mut vga, 6, 800);
            svga::set(&mut vga, 8, 5);
            svga::set(&mut vga, 9, 40);
            svga::set(&mut vga, 5, 3);
            let before = (vga.core_state(), vga.dispi_state(), vga.dac_state());
            let pixel = {
                let mut frame = vga.framebuffer();
                let pixel = frame.pixel(0, 1).unwrap();
                assert_eq!((pixel.color_mask(), pixel.format()), (mask, format));
                frame.write_pixel(&pixel, value);
                pixel
            };
            assert_eq!(
                (vga.core_state(), vga.dispi_state(), vga.dac_state()),
                before
            );
            let address = (41 * 800 + 5) * u32::from(bits / 8);
            svga::set(&mut vga, 5, (address / 65536) as u16);
            assert_eq!(
                vga.read_mmio(
                    context.reborrow(),
                    0xa0000 + address % 65536,
                    (bits / 8) as u8
                ),
                u64::from(value)
            );
            vga.framebuffer().write_pixel(&pixel, pixel.value());
            assert_eq!(
                vga.read_mmio(
                    context.reborrow(),
                    0xa0000 + address % 65536,
                    (bits / 8) as u8
                ),
                0
            );
        }
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn cga_interleaved_pixels_preserve_packed_neighbors_in_the_odd_scanline_bank() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let mut vga = Vga::with_clock(ManualClock::new().clock());
        indexed256::program(&mut vga);
        for (port, register, value) in [
            (0x3c4, 1, 9),
            (0x3c4, 2, 3),
            (0x3c4, 4, 2),
            (0x3ce, 5, 0x30),
            (0x3ce, 6, 0x0f),
            (0x3d4, 1, 39),
            (0x3d4, 9, 0xc1),
            (0x3d4, 0x13, 20),
            (0x3d4, 0x14, 0),
            (0x3d4, 0x17, 0xa2),
        ] {
            indexed(&mut vga, port, register, value);
        }
        support::registers::attribute(&mut vga, 0x10, 1);
        support::registers::attribute(&mut vga, 0x12, 3);
        vga.write_mmio(context.reborrow(), 0xba001, 1, 0x6c); // Four two-bit pixels: 1, 2, 3, 0.
        let pixel = {
            let mut frame = vga.framebuffer();
            assert_eq!(
                frame.geometry(),
                Some(FramebufferGeometry::Graphics {
                    width: 320,
                    height: 200
                })
            );
            let pixel = frame.pixel(5, 1).unwrap();
            assert_eq!((pixel.value(), pixel.color_mask()), (2, 3));
            frame.write_pixel(&pixel, 3);
            pixel
        };
        assert_eq!(vga.read_mmio(context.reborrow(), 0xba001, 1), 0x7c);
        assert_eq!(vga.read_mmio(context.reborrow(), 0xb8001, 1), 0);
        vga.framebuffer().write_pixel(&pixel, pixel.value());
        assert_eq!(vga.read_mmio(context.reborrow(), 0xba001, 1), 0x6c);
    })
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn serializer_rotation_is_inverted_for_native_planar_and_indexed_writes() {
    relocade_machine::IoBus::default().with_context(|mut context| {
        let mut vga = Vga::with_clock(ManualClock::new().clock());
        planar16::program(&mut vga);
        indexed(&mut vga, 0x3c4, 2, 1);
        vga.write_mmio(context.reborrow(), 0xa0000, 1, 0x80);
        indexed(&mut vga, 0x3c4, 1, 5); // Reload every second character clock.
        let pixel = vga.framebuffer().pixel(8, 0).unwrap();
        assert_eq!(pixel.value(), 8);
        vga.framebuffer().write_pixel(&pixel, 3);
        let snapshot = vga.snapshot();
        assert_eq!(
            [
                snapshot.plane(0)[0],
                snapshot.plane(1)[0],
                snapshot.plane(2)[0]
            ],
            [0, 0x80, 0x80]
        );
        vga.framebuffer().write_pixel(&pixel, 0x10); // Bits outside the four-bit format are ignored.
        assert_eq!(vga.framebuffer().pixel(8, 0).unwrap().value(), 0);
        vga.framebuffer().write_pixel(&pixel, pixel.value());
        assert_eq!(vga.snapshot().plane(0)[0], 0x80);
        assert_eq!(vga.snapshot().plane(1)[0], 0);

        indexed256::program(&mut vga);
        vga.write_mmio(context.reborrow(), 0xa0000, 1, 0xa5);
        indexed(&mut vga, 0x3c4, 1, 0x11); // Reload every fourth character clock.
        let pixel = vga.framebuffer().pixel(4, 0).unwrap();
        assert_eq!(pixel.value(), 0x5a);
        vga.framebuffer().write_pixel(&pixel, 0x36);
        assert_eq!(vga.read_mmio(context.reborrow(), 0xa0000, 1), 0x6c);
        vga.framebuffer().write_pixel(&pixel, pixel.value());
        assert_eq!(vga.read_mmio(context.reborrow(), 0xa0000, 1), 0xa5);
    })
}
