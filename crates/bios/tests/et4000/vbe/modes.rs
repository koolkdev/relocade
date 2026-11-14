use super::{call, Firmware};
use relocade_vga::{FramebufferAccess, FramebufferGeometry};

async fn banked_modes() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    assert_eq!(
        call(&mut cpu, 0x4f05, 0, 0, 1).await,
        0x014f,
        "text has no VBE banks"
    );
    for (number, width, height) in [(0x101, 640, 480), (0x103, 800, 600), (0x105, 1024, 768)] {
        assert_eq!(call(&mut cpu, 0x4f02, number, 0, 0).await, 0x004f);
        assert_eq!(
            machine.geometry(),
            FramebufferGeometry::Graphics { width, height }
        );
        assert_eq!(call(&mut cpu, 0x4f03, 0, 0, 0).await, 0x004f);
        assert_eq!(cpu.cpu_state().bx(), number);
    }
    assert_eq!(call(&mut cpu, 0x4f02, 0x101, 0, 0).await, 0x004f);
    assert_eq!(call(&mut cpu, 0x4f05, 0, 0, 1).await, 0x004f);
    machine.io.write_mmio(0xa0000, 1, 0x5a);
    assert_eq!(
        machine.io.read_mmio(0xa0000, 1),
        0,
        "read bank remains zero"
    );
    assert_eq!(call(&mut cpu, 0x4f05, 1, 0, 1).await, 0x004f);
    assert_eq!(machine.io.read_mmio(0xa0000, 1), 0x5a);
    assert_eq!(
        machine
            .card
            .borrow_mut()
            .framebuffer()
            .pixel(256, 102)
            .unwrap()
            .value(),
        0x5a
    );
    assert_eq!(call(&mut cpu, 0x4f05, 0, 0, 15).await, 0x004f);
    machine.io.write_mmio(0xaffff, 1, 0x7c); // Last physical VRAM byte.

    let before = machine.card.borrow().core_state();
    for operation in [2, 0x102, 0x200, 0xffff] {
        assert_eq!(call(&mut cpu, 0x4f05, operation, 0, 0).await, 0x014f);
    }
    assert_eq!(call(&mut cpu, 0x4f05, 0, 0, 16).await, 0x014f);
    for number in [0x4101, 0x111, 0x105 | 0x2000, 0x39] {
        assert_eq!(call(&mut cpu, 0x4f02, number, 0, 0).await, 0x014f);
    }
    assert_eq!(machine.card.borrow().core_state(), before);
    assert_eq!(machine.io.read_port(0x3cd, 1), 0x1f);
    assert_eq!(call(&mut cpu, 0x4f05, 0x100, 0, 0).await, 0x004f);
    assert_eq!(cpu.cpu_state().dx(), 15);
    assert_eq!(call(&mut cpu, 0x4f05, 0x101, 0, 0).await, 0x004f);
    assert_eq!(cpu.cpu_state().dx(), 1);

    // Preservation covers all VRAM, and mode entry resets both windows.
    assert_eq!(call(&mut cpu, 0x4f02, 0x8101, 0, 0).await, 0x004f);
    assert_eq!(machine.io.read_port(0x3cd, 1), 0);
    assert_eq!(call(&mut cpu, 0x4f05, 1, 0, 15).await, 0x004f);
    assert_eq!(machine.io.read_mmio(0xaffff, 1), 0x7c);
    assert_eq!(call(&mut cpu, 0x4f02, 0x101, 0, 0).await, 0x004f);
    assert_eq!(call(&mut cpu, 0x4f05, 1, 0, 15).await, 0x004f);
    assert_eq!(machine.io.read_mmio(0xaffff, 1), 0);
    assert_eq!(call(&mut cpu, 0x4f02, 3, 0, 0).await, 0x004f);
    assert_eq!(call(&mut cpu, 0x4f03, 0, 0, 0).await, 0x004f);
    assert_eq!(cpu.cpu_state().bx(), 3);
    assert_eq!(
        machine.geometry(),
        FramebufferGeometry::Text {
            columns: 80,
            rows: 25
        }
    );
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn vbe_modes_and_independent_windows_program_the_connected_card() {
    pollster::block_on(banked_modes());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn vbe_modes_and_independent_windows_program_the_connected_card() {
    banked_modes().await;
}
