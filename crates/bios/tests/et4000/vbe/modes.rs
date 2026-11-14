use super::{call, execution, information, Firmware};
use relocade_machine::{real_mode_interrupt::Interrupts, FarPointer};
use relocade_vga::{FramebufferAccess, FramebufferGeometry, PixelFormat};
use relocade_x86::{ExecutionExit, Segment, SegmentRegister};

async fn banked_modes() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    assert_eq!(
        call(&mut cpu, 0x4f05, 0, 0, 1).await,
        0x014f,
        "text has no VBE banks"
    );
    for (number, width, height) in [
        (0x101, 640, 480),
        (0x103, 800, 600),
        (0x105, 1024, 768),
        (0x110, 640, 480),
        (0x113, 800, 600),
    ] {
        assert_eq!(call(&mut cpu, 0x4f02, number, 0, 0).await, 0x004f);
        assert_eq!(
            machine.geometry(),
            FramebufferGeometry::Graphics { width, height }
        );
        assert_eq!(call(&mut cpu, 0x4f03, 0, 0, 0).await, 0x004f);
        assert_eq!(cpu.cpu_state().bx(), number);
        let layout = machine.card.borrow().packed_layout().unwrap();
        let high_color = matches!(number, 0x110 | 0x113);
        assert_eq!(
            layout.format(),
            if high_color {
                PixelFormat::Rgb555
            } else {
                PixelFormat::Indexed8
            }
        );
        assert_eq!(
            layout.pitch(),
            u32::from(width) * if high_color { 2 } else { 1 }
        );
        if high_color {
            machine.io.write_port(0x3c4, 1, 7);
            assert_eq!(machine.io.read_port(0x3c5, 1) & 0x40, 0x40);
            machine.io.write_port(0x3d4, 1, 0x35);
            assert_eq!(machine.io.read_port(0x3d5, 1) & 0x80, 0x80);
        }
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
    for number in [0x4101, 0x111, 0x116, 0x105 | 0x2000, 0x39] {
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

    assert_eq!(call(&mut cpu, 0x4f02, 0x110, 0, 0).await, 0x004f);
    // A word on either side of the bank boundary is still two physical bytes.
    machine.io.write_mmio(0xafffe, 2, 0x801f);
    assert_eq!(call(&mut cpu, 0x4f05, 0, 0, 1).await, 0x004f);
    machine.io.write_mmio(0xa0000, 2, 0x03e0);
    {
        let mut card = machine.card.borrow_mut();
        let framebuffer = card.framebuffer();
        assert_eq!(framebuffer.pixel(127, 51).unwrap().value(), 0x801f);
        assert_eq!(framebuffer.pixel(128, 51).unwrap().value(), 0x03e0);
    }
    assert_eq!(call(&mut cpu, 0x4f02, 0x8110, 0, 0).await, 0x004f);
    assert_eq!(machine.io.read_mmio(0xafffe, 2), 0x801f);
    assert_eq!(call(&mut cpu, 0x4f02, 0x8101, 0, 0).await, 0x004f);
    assert_eq!(
        machine.card.borrow().packed_layout().unwrap().format(),
        PixelFormat::Indexed8
    );
    assert_eq!(machine.io.read_mmio(0xafffe, 2), 0x801f);
    assert_eq!(call(&mut cpu, 0x4f02, 0x110, 0, 0).await, 0x004f);
    assert_eq!(machine.io.read_mmio(0xafffe, 2), 0);
    assert_eq!(call(&mut cpu, 0x4f02, 3, 0, 0).await, 0x004f);
    machine.io.read_port(0x3c8, 1);
    for _ in 0..4 {
        machine.io.read_port(0x3c6, 1);
    }
    assert_eq!(
        machine.io.read_port(0x3c6, 1),
        0,
        "text mode resets high color"
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

async fn far_call_windows() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    assert_eq!(call(&mut cpu, 0x4f02, 0x101, 0, 0).await, 0x004f);
    let (status, bytes) = information(&machine, &mut cpu, 0x4f01, 0x101).await;
    assert_eq!(status, 0x004f);
    let window = FarPointer::from_packed(u32::from_le_bytes(bytes[12..16].try_into().unwrap()));
    let mut program = vec![0x9a]; // CALL FAR WinFuncPtr; JMP $
    program.extend_from_slice(&window.packed().to_le_bytes());
    program.extend_from_slice(&[0xeb, 0xfe]);
    machine.memory.view().write(0x110, &program).unwrap();

    // A direct BIOS procedure must remain callable after a guest replaces INT 10h.
    let mut interrupts = Interrupts::new(&mut cpu);
    interrupts.register_interrupt(
        0x10,
        FarPointer {
            segment: 0,
            offset: 0x300,
        },
        async |_| panic!("WinFuncPtr must bypass the INT 10h vector"),
    );
    interrupts.install().await.unwrap();

    let state = cpu.cpu_state();
    state.set_cx(0x1234);
    state.set_si(0x5678);
    state.set_di(0x9abc);
    state.set_bp(0xdef0);
    state.set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x1234));
    state.set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0x5678));
    for (operation, position, banks) in [
        (0, 3, 0x03),
        (1, 5, 0x53),
        (0x100, 0xffff, 0x53),
        (0x101, 0xffff, 0x53),
    ] {
        state.set_eip(0x110);
        state.set_ax(0xdead); // Far callers need not supply AX=4F05h.
        state.set_bx(operation);
        state.set_dx(position);
        state.set_eflags(0x443); // Preserve arithmetic flags, DF and cleared IF.
        let before = state.snapshot();
        assert_eq!(
            execution::run_until(&mut cpu, 0x115).await,
            ExecutionExit::Interrupted
        );
        let after = state.snapshot();
        assert_eq!(machine.io.read_port(0x3cd, 1), banks);
        if operation == 0x100 {
            assert_eq!(state.dx(), 3);
        } else if operation == 0x101 {
            assert_eq!(state.dx(), 5);
        }
        // AX/DX may change; all other registers, the stack and flags survive RETF.
        let mut expected = before.registers;
        expected.eax = after.registers.eax;
        expected.edx = after.registers.edx;
        assert_eq!(after.registers, expected);
        assert_eq!(after.segments, before.segments);
        assert_eq!(after.flags.eflags(), before.flags.eflags());
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn vbe_window_pointer_accepts_far_calls_without_an_ax_function_number() {
    pollster::block_on(far_call_windows());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn vbe_window_pointer_accepts_far_calls_without_an_ax_function_number() {
    far_call_windows().await;
}
