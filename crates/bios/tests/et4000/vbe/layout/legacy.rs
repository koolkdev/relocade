use super::{call, Firmware};
use relocade_vga::FramebufferAccess;

async fn scanlines() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    assert_eq!(call(&mut cpu, 0x4f06, 1, 0, 0).await, 0x004f);
    let state = cpu.cpu_state();
    assert_eq!((state.bx(), state.cx(), state.dx()), (160, 720, 3264));
    assert_eq!(call(&mut cpu, 0x4f06, 0, 721, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (164, 738, 3184));
    machine.io.write_mmio(0xb8000 + 164, 2, 0x0741);
    assert_eq!(
        machine
            .card
            .borrow_mut()
            .framebuffer()
            .text_cell(1, 0)
            .unwrap()
            .value(),
        0x0741
    );
    machine.set_mode(0x0f).unwrap(); // Monochrome CRTC routing.
    assert_eq!(call(&mut cpu, 0x4f06, 0, 800, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (100, 800, 2621));
    assert_eq!(
        machine.card.borrow().registers().crtc.registers.values[0x13],
        50
    );
    machine.set_mode(0x13).unwrap();
    assert_eq!(call(&mut cpu, 0x4f06, 1, 0, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (320, 320, 3276));
}

async fn extended_scanlines() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    let state = cpu.cpu_state();
    machine.set_mode(0x12).unwrap();
    machine.io.write_port(0x3bf, 1, 3);
    machine.io.write_port(0x3d8, 1, 0xa0);
    assert_eq!(call(&mut cpu, 0x4f06, 2, 100, 0).await, 0x004f);
    // Row 655 begins just below counter 10000h. A wider pitch crosses it.
    for value in [0x0c33, 0xff0c, 0xdc0d] {
        machine.io.write_port(0x3d4, 2, value);
    }
    machine.io.write_port(0x3cd, 1, 0x75);
    assert_eq!(call(&mut cpu, 0x4f06, 2, 102, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (102, 816, 2570));
    assert_eq!(
        &machine.card.borrow().registers().crtc.registers.values[0x0c..0x0e],
        &[4, 0xfa]
    );
    machine.io.write_port(0x3d4, 1, 0x33);
    assert_eq!(machine.io.read_port(0x3d5, 1), 0x0d); // Cursor high bits survive.
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x75);
    machine.io.write_port(0x3c4, 2, 0x0102);
    machine.io.write_port(0x3cd, 1, 0x71);
    machine.io.write_mmio(0xa04fa, 1, 0x80);
    machine.io.write_port(0x3cd, 1, 0x75);
    assert_eq!(
        machine
            .card
            .borrow_mut()
            .framebuffer()
            .pixel(0, 0)
            .unwrap()
            .value(),
        1
    );
    // Shrinking back must use the full programmed origin and clear its high bit.
    assert_eq!(call(&mut cpu, 0x4f06, 2, 100, 0).await, 0x004f);
    assert_eq!(
        &machine.card.borrow().registers().crtc.registers.values[0x0c..0x0e],
        &[0xff, 0xdc]
    );
    machine.io.write_port(0x3d4, 1, 0x33);
    assert_eq!(machine.io.read_port(0x3d5, 1), 0x0c);

    // A 640x1026 raster uses CR35's display-height and line-compare extensions.
    machine.set_mode(0x12).unwrap();
    machine.io.write_port(0x3bf, 1, 3);
    machine.io.write_port(0x3d8, 1, 0xa0);
    for value in [0x0011, 0x1007, 0x4009, 0x0112, 0x1435] {
        machine.io.write_port(0x3d4, 2, value);
    }
    assert_eq!(call(&mut cpu, 0x4f06, 3, 0, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (254, 2032, 1032));
    // Clearing just the extended split bit puts a split inside the raster.
    machine.io.write_port(0x3d4, 2, 0x0435);
    let before = machine.card.borrow().core_state();
    let extended = machine.card.borrow().et4000_state();
    assert_eq!(call(&mut cpu, 0x4f06, 1, 0, 0).await, 0x014f);
    assert_eq!(call(&mut cpu, 0x4f06, 2, 100, 0).await, 0x014f);
    assert_eq!(machine.card.borrow().core_state(), before);
    assert_eq!(machine.card.borrow().et4000_state(), extended);

    machine.io.write_port(0x3d4, 2, 0x1435);
    machine.io.write_port(0x3d4, 2, 0xa317); // IBM word mapping is not a linear plane.
    assert_eq!(call(&mut cpu, 0x4f06, 1, 0, 0).await, 0x014f);
    machine.io.write_port(0x3d4, 2, 0x6036); // Tseng contiguous mapping ignores word mode.
    assert_eq!(call(&mut cpu, 0x4f06, 3, 0, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (254, 2032, 1032));
    machine.io.write_port(0x3d4, 2, 0x7036); // Relocated host aperture is unconnected.
    let before = machine.card.borrow().core_state();
    let extended = machine.card.borrow().et4000_state();
    assert_eq!(call(&mut cpu, 0x4f06, 2, 100, 0).await, 0x014f);
    assert_eq!(machine.card.borrow().core_state(), before);
    assert_eq!(machine.card.borrow().et4000_state(), extended);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn scanlines_preserve_full_start_and_respect_extended_geometry() {
    pollster::block_on(extended_scanlines());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn scanlines_preserve_full_start_and_respect_extended_geometry() {
    extended_scanlines().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn legacy_scanlines_use_aperture_or_banked_capacity() {
    pollster::block_on(scanlines());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn legacy_scanlines_use_aperture_or_banked_capacity() {
    scanlines().await;
}
