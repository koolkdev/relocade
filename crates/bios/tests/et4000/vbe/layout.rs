use super::{call, Firmware};

async fn viewport_services() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    assert_eq!(call(&mut cpu, 0x4f06, 1, 0, 0).await, 0x014f);
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
