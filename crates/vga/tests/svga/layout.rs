use crate::common::fixture::ConnectedVga;
use crate::common::support::svga::{enable, get, set};
use relocade_clock_test_support::ManualClock;
use relocade_vga::{PixelFormat, SvgaMode, SvgaResolution};

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn layout_bounds_keep_coordinates_and_full_logical_rows_in_vram() {
    let layout = SvgaMode::new(SvgaResolution::R640x480, PixelFormat::Indexed8)
        .layout()
        .with_pitch(1024)
        .unwrap();
    assert_eq!(layout.virtual_height(), 4096);
    assert_eq!(layout.image_pages(), 8);
    let corner = layout.with_origin([384, 3616]).unwrap();
    assert_eq!(
        corner.display_start() + 479 * 1024 + 639,
        4 * 1024 * 1024 - 1
    );
    for origin in [[385, 3616], [384, 3617], [u32::MAX, 0], [0, u32::MAX]] {
        assert!(layout.with_origin(origin).is_none());
    }
    for pitch in [0, 639, 8739, u32::MAX] {
        assert!(layout.with_pitch(pitch).is_none());
    }
    assert!(corner.with_pitch(1023).is_none());
    assert!(corner.with_pitch(1025).is_none());
    let panned = layout.with_origin([32, 10]).unwrap();
    let wider = panned.with_pitch(2048).unwrap();
    assert_eq!(wider.origin(), [32, 10]);
    assert_eq!(wider.display_start(), 20512);

    // The final 1024 bytes cannot provide a full logical row at this pitch.
    let partial_row = layout.with_pitch(1280).unwrap();
    assert_eq!(partial_row.virtual_height(), 3276);
    assert!(partial_row.with_origin([0, 2796]).is_some());
    assert!(partial_row.with_origin([0, 2797]).is_none());
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn direct_color_layouts_separate_pixel_coordinates_from_byte_offsets() {
    for (format, pitch, rows, start, max_pitch) in [
        (PixelFormat::Rgb565, 2048, 2048, 3_212_032, 8738),
        (PixelFormat::Bgrx8888, 4096, 1024, 2_229_760, 8736),
    ] {
        let mode = SvgaMode::new(SvgaResolution::R640x480, format);
        let layout = mode.layout().with_virtual_width(1024).unwrap();
        assert_eq!(layout.pitch(), pitch);
        assert_eq!(layout.virtual_width(), 1024);
        assert_eq!(layout.virtual_height(), rows);
        assert_eq!(mode.max_pitch(), max_pitch);
        let corner = layout.with_origin([384, rows - 480]).unwrap();
        assert_eq!(corner.display_start(), start);
        assert_eq!(
            corner.display_start() + 479 * pitch + 640 * format.bytes_per_pixel(),
            4 * 1024 * 1024
        );
        assert!(layout.with_origin([385, rows - 480]).is_none());
        assert!(layout.with_origin([384, rows - 479]).is_none());
        assert!(layout.with_pitch(pitch + 1).is_none());
        assert!(layout.with_pitch(max_pitch).is_some());
        assert!(layout
            .with_pitch(max_pitch + format.bytes_per_pixel())
            .is_none());
        assert!(layout.with_virtual_width(639).is_none());
        assert!(layout.with_virtual_width(u32::MAX).is_none());
        assert!(corner.with_virtual_width(1023).is_none());
    }
}

#[cfg_attr(not(target_arch = "wasm32"), test)]
#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
fn virtual_registers_validate_without_changing_the_cpu_bank_or_memory() {
    let vga = ConnectedVga::new(ManualClock::new().clock());
    set(&vga.io, 6, 1024); // Inactive layout writes are ignored.
    assert_eq!(get(&vga.io, 6), 0);
    enable(&vga.io);
    set(&vga.io, 5, 63);
    vga.io.write_mmio(0xaffff, 1, 0xa5);
    set(&vga.io, 6, 1024);
    set(&vga.io, 8, 384);
    set(&vga.io, 9, 3616);
    for (index, value) in [(6, 1023), (6, 1025), (7, 1), (8, 385), (9, 3617)] {
        set(&vga.io, index, value);
    }
    assert_eq!(
        (
            get(&vga.io, 6),
            get(&vga.io, 7),
            get(&vga.io, 8),
            get(&vga.io, 9)
        ),
        (1024, 4096, 384, 3616)
    );
    assert_eq!(get(&vga.io, 5), 63);
    assert_eq!(vga.io.read_mmio(0xaffff, 1), 0xa5);
    set(&vga.io, 4, 0);
    set(&vga.io, 4, 0x81);
    assert_eq!(
        (get(&vga.io, 6), get(&vga.io, 8), get(&vga.io, 9)),
        (640, 0, 0)
    );
    set(&vga.io, 5, 63);
    assert_eq!(vga.io.read_mmio(0xaffff, 1), 0xa5);
}
