use super::{execution, Firmware};
use relocade_bios::Keyboard;
use relocade_machine::{real_mode_interrupt::Interrupts, Cpu, CpuConfig, FarPointer};
use relocade_x86::{Compiler, CpuState, ExecutionExit, ExecutionProfile, Segment, SegmentRegister};

impl Firmware {
    async fn cpu(&self) -> Cpu {
        let mut state = CpuState::real_mode();
        state.registers.esp = 0x8000;
        state.flags.set_eflags(2);
        let config =
            CpuConfig::new(Compiler::new(ExecutionProfile::Real16).target(), state).unwrap();
        let mut cpu = Cpu::new(self.memory.clone(), config, self.io.clone())
            .await
            .unwrap();
        let mut interrupts = Interrupts::new(&mut cpu);
        relocade_bios::register_interrupts(
            &mut interrupts,
            Keyboard::new(self.memory.clone()),
            self.firmware.clone(),
            None,
        );
        interrupts.install().await.unwrap();
        self.memory
            .view()
            .write(0x100, &[0xcd, 0x10, 0xeb, 0xfe])
            .unwrap();
        cpu
    }
}

async fn call(cpu: &mut Cpu, ax: u16, bx: u16, cx: u16, dx: u16) -> u16 {
    let state = cpu.cpu_state();
    state.set_eip(0x100);
    state.set_ax(ax);
    state.set_bx(bx);
    state.set_cx(cx);
    state.set_dx(dx);
    assert_eq!(
        execution::run_until(cpu, 0x102).await,
        ExecutionExit::Interrupted
    );
    state.ax()
}

async fn information(
    machine: &Firmware,
    cpu: &mut Cpu,
    function: u16,
    number: u16,
) -> (u16, [u8; 256]) {
    let destination = FarPointer {
        segment: 0x5000,
        offset: 0xff80,
    };
    let state = cpu.cpu_state();
    state.set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, destination.segment),
    );
    state.set_di(destination.offset);
    let mut memory = machine.memory.view();
    memory
        .write_wrapping(destination.wrapping_add(0xffff), &[0xa5; 258])
        .unwrap();
    memory.write_wrapping(destination, b"VBE2").unwrap();
    let status = call(cpu, function, 0x1234, number, 0x5678).await;
    assert_eq!(state.bx(), 0x1234);
    assert_eq!(state.dx(), 0x5678);
    assert_eq!(
        memory
            .read_u8(destination.wrapping_add(0xffff).address())
            .unwrap(),
        0xa5
    );
    assert_eq!(
        memory
            .read_u8(destination.wrapping_add(256).address())
            .unwrap(),
        0xa5
    );
    let mut bytes = [0; 256];
    memory.read_wrapping(destination, &mut bytes).unwrap();
    (status, bytes)
}

async fn discovery() {
    let machine = Firmware::new();
    let mut cpu = machine.cpu().await;
    let before = machine.card.borrow().core_state();
    let (status, bytes) = information(&machine, &mut cpu, 0x4f00, 0).await;
    assert_eq!(status, 0x004f);
    assert_eq!(&bytes[..6], b"VESA\x02\x01");
    assert_eq!(&bytes[18..20], &[16, 0]); // One MiB, not DISPI's four.
    let list = FarPointer::from_packed(u32::from_le_bytes(bytes[14..18].try_into().unwrap()));
    let mut modes = [0; 8];
    machine
        .memory
        .view()
        .read_wrapping(list, &mut modes)
        .unwrap();
    assert_eq!(modes, [1, 1, 3, 1, 5, 1, 0xff, 0xff]);

    for (number, width, height, pages) in [
        (0x101, 640u16, 480u16, 2),
        (0x103, 800, 600, 1),
        (0x105, 1024, 768, 0),
    ] {
        let (status, bytes) = information(&machine, &mut cpu, 0x4f01, number).await;
        assert_eq!(status, 0x004f);
        // Independent write/read windows, same aperture, no callable window pointer.
        assert_eq!(
            &bytes[..16],
            &[0x1b, 0, 5, 3, 64, 0, 64, 0, 0, 0xa0, 0, 0xa0, 0, 0, 0, 0]
        );
        assert_eq!(&bytes[16..18], &width.to_le_bytes());
        assert_eq!(&bytes[18..20], &width.to_le_bytes());
        assert_eq!(&bytes[20..22], &height.to_le_bytes());
        assert_eq!(&bytes[25..31], &[8, 1, 4, 0, pages, 1]);
    }
    for number in [0x111, 0x107, 0x4101, 0x8101] {
        let (status, bytes) = information(&machine, &mut cpu, 0x4f01, number).await;
        assert_eq!(status, 0x014f);
        assert_eq!(&bytes[..4], b"VBE2");
        assert_eq!(&bytes[4..], &[0xa5; 252]);
    }
    assert_eq!(machine.card.borrow().core_state(), before);

    // Guest output buffers may themselves be in enabled video memory. The
    // serializer must release its device borrow before entering MMIO callbacks.
    let state = cpu.cpu_state();
    state.set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0xb800));
    state.set_di(0);
    assert_eq!(call(&mut cpu, 0x4f00, 0, 0, 0).await, 0x004f);
    assert_eq!(
        machine.io.read_mmio(0xb8000, 4),
        u64::from(u32::from_le_bytes(*b"VESA"))
    );
    assert_eq!(call(&mut cpu, 0x4f01, 0, 0x101, 0).await, 0x004f);
    assert_eq!(machine.io.read_mmio(0xb8000, 4), 0x0305_001b);

    // A legal wrapping scanout can be larger than physical VRAM. It has no
    // additional complete pages, even though the active pitch is still reported.
    machine.set_mode(0x38).unwrap();
    machine.io.write_port(0x3d4, 2, 0xff13);
    let (status, bytes) = information(&machine, &mut cpu, 0x4f01, 0x105).await;
    assert_eq!(status, 0x004f);
    assert_eq!(&bytes[16..18], &2040u16.to_le_bytes());
    assert_eq!(bytes[29], 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn vbe_queries_describe_the_selected_board_without_changing_it() {
    pollster::block_on(discovery());
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen_test::wasm_bindgen_test]
async fn vbe_queries_describe_the_selected_board_without_changing_it() {
    discovery().await;
}
