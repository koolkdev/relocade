use super::{call, Firmware};

async fn viewport_services() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    assert_eq!(call(&mut cpu, 0x4f06, 1, 0, 0).await, 0x004f);
    let state = cpu.cpu_state();
    assert_eq!((state.bx(), state.cx(), state.dx()), (160, 720, 3264));
    assert_eq!(call(&mut cpu, 0x4f06, 0, 721, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (164, 738, 3184));
    machine.io.write_mmio(0xb8000 + 164, 2, 0x0741);
    assert_eq!(
        relocade_vga::FramebufferAccess::framebuffer(&mut *machine.card.borrow_mut())
            .text_cell(1, 0)
            .unwrap()
            .value(),
        0x0741
    );
    machine.set_mode(0x0f).unwrap(); // Monochrome CRTC routing.
    assert_eq!(call(&mut cpu, 0x4f06, 0, 800, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (100, 800, 655));
    assert_eq!(
        machine.card.borrow().registers().crtc.registers.values[0x13],
        50
    );
    machine.set_mode(0x13).unwrap();
    assert_eq!(call(&mut cpu, 0x4f06, 1, 0, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (320, 320, 204));
    assert_eq!(call(&mut cpu, 0x4f02, 0x101, 0, 0).await, 0x004f);
    let state = cpu.cpu_state();
    assert_eq!(call(&mut cpu, 0x4f06, 0, 1021, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (1024, 1024, 1024));
    machine.io.write_port(0x3cd, 1, 0x75);
    assert_eq!(call(&mut cpu, 0x4f07, 0, 3, 500).await, 0x004f);
    assert_eq!(call(&mut cpu, 0x4f07, 1, 0, 0).await, 0x004f);
    assert_eq!((state.cx(), state.dx()), (3, 500));
    let registers = machine.card.borrow().registers().crtc.registers.values;
    assert_eq!(registers[0x0c], 0xf4);
    assert_eq!(registers[0x0d], 0x00);
    machine.io.write_port(0x3d4, 1, 0x33);
    assert_eq!(machine.io.read_port(0x3d5, 1) & 3, 1);
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x75);
    assert_eq!(call(&mut cpu, 0x4f06, 2, 797, 0).await, 0x004f);
    assert_eq!(state.bx(), 800);
    assert_eq!(
        machine.card.borrow().packed_layout().unwrap().origin(),
        [3, 500]
    );

    let before = machine.card.borrow().core_state();
    for (function, operation, x, y, error) in [
        (0x4f06, 0, 2041, 0, 0x024f),
        (0x4f06, 4, 0, 0, 0x014f),
        (0x4f07, 0, 161, 0, 0x014f),
        (0x4f07, 0, 0, 831, 0x014f),
        (0x4f07, 0x100, 0, 0, 0x014f),
    ] {
        assert_eq!(call(&mut cpu, function, operation, x, y).await, error);
    }
    assert_eq!(machine.card.borrow().core_state(), before);
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x75);
    assert_eq!(call(&mut cpu, 0x4f06, 3, 0, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (2040, 2040, 514));
    assert_eq!(call(&mut cpu, 0x4f08, 0x0800, 0, 0).await, 0x004f);
    assert_eq!(state.bh(), 6);
    assert_eq!(call(&mut cpu, 0x4f08, 0x0500, 0, 0).await, 0x014f);
    assert_eq!(call(&mut cpu, 0x4f02, 0x105, 0, 0).await, 0x004f);
    assert_eq!(call(&mut cpu, 0x4f06, 3, 0, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (1360, 1360, 771));

    // RGB555 counts 15 color bits but occupies two bytes per pixel.
    assert_eq!(call(&mut cpu, 0x4f02, 0x110, 0, 0).await, 0x004f);
    assert_eq!(call(&mut cpu, 0x4f06, 0, 641, 0).await, 0x004f);
    assert_eq!((state.bx(), state.cx(), state.dx()), (1288, 644, 814));
    assert_eq!(call(&mut cpu, 0x4f07, 0, 1, 1).await, 0x004f);
    assert_eq!(call(&mut cpu, 0x4f07, 1, 0, 0).await, 0x004f);
    assert_eq!((state.cx(), state.dx()), (1, 1));
    assert_eq!(
        machine
            .card
            .borrow()
            .packed_layout()
            .unwrap()
            .display_start(),
        1290
    );
    let (_, indexed) = super::information(&machine, &mut cpu, 0x4f01, 0x101).await;
    assert_eq!(&indexed[16..18], &640u16.to_le_bytes());
    let (_, high_color) = super::information(&machine, &mut cpu, 0x4f01, 0x110).await;
    assert_eq!(&high_color[16..18], &1288u16.to_le_bytes());
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn vbe_viewports_respect_crtc_units_capacity_and_independent_banks() {
    pollster::block_on(viewport_services());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn vbe_viewports_respect_crtc_units_capacity_and_independent_banks() {
    viewport_services().await;
}

async fn legacy_display_start() {
    use relocade_vga::FramebufferAccess;
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    let state = cpu.cpu_state();
    machine.set_mode(0x0f).unwrap();
    assert_eq!(call(&mut cpu, 0x4f06, 2, 100, 0).await, 0x4f);
    machine.io.write_port(0x3c4, 2, 0x0102);
    machine.io.write_mmio(0xa0000 + 501, 1, 4);
    assert_eq!(call(&mut cpu, 0x4f07, 0, 13, 5).await, 0x4f);
    assert_eq!(call(&mut cpu, 0x4f07, 1, 0, 0).await, 0x4f);
    assert_eq!((state.cx(), state.dx()), (13, 5));
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

    machine.set_mode(0x13).unwrap();
    machine.io.write_port(0x3d4, 2, 0x6712);
    assert_eq!(call(&mut cpu, 0x4f06, 0, 321, 0).await, 0x4f);
    machine.io.write_mmio(0xa0000 + 659, 1, 0x5a);
    assert_eq!(call(&mut cpu, 0x4f07, 0, 3, 2).await, 0x4f);
    assert_eq!(
        machine
            .card
            .borrow_mut()
            .framebuffer()
            .pixel(0, 0)
            .unwrap()
            .value(),
        0x5a
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn vbe_legacy_panning_uses_monochrome_routing_and_et4000_chained_addressing() {
    pollster::block_on(legacy_display_start());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn vbe_legacy_panning_uses_monochrome_routing_and_et4000_chained_addressing() {
    legacy_display_start().await;
}
