use super::{common, success, with_mouse, Dos};
pub(super) use common::mouse::inject;
use relocade_x86::{ExecutionExit, Segment, SegmentRegister};

pub(super) async fn install(dos: &mut Dos) {
    let memory = dos.cpu().memory();
    let segment = dos.process().unwrap().psp_segment();
    let base = u32::from(segment) << 4;
    // Read the actual callback stack into CS-relative storage. The callee owns
    // no argument cleanup and deliberately clobbers registers before RETF.
    memory
        .view()
        .write(
            base + 0x300,
            &[
                0x89, 0xe5, // MOV BP,SP.
                0x8b, 0x46, 4, 0x2e, 0xa3, 0, 5, // Z -> 0500h.
                0x8b, 0x46, 6, 0x2e, 0xa3, 2, 5, // Y -> 0502h.
                0x8b, 0x46, 8, 0x2e, 0xa3, 4, 5, // X -> 0504h.
                0x8b, 0x46, 10, 0x2e, 0xa3, 6, 5, // Status -> 0506h.
                0x2e, 0xff, 0x06, 8, 5, // Increment callback count.
                0x66, 0xb8, 0xef, 0xbe, 0xad, 0xde, // EAX=DEADBEEFh.
                0x8e, 0xd8, 0x8e, 0xc0, // DS=ES=BEEFh.
                0x89, 0xc3, 0x89, 0xc1, 0x89, 0xc2, // BX=CX=DX=AX.
                0x89, 0xc6, 0x89, 0xc7, 0x89, 0xc5, // SI=DI=BP=AX.
                0xcb, // RETF leaves all four words for BIOS cleanup.
            ],
        )
        .unwrap();
    dos.cpu().cpu_state().set_segment(
        Segment::Es,
        SegmentRegister::real_mode(Segment::Es, segment),
    );
    success(dos, 5, 0x0300).await;
    success(dos, 7, 0x0300).await;
    success(dos, 0, 0x0100).await;
}

pub(super) async fn deliver(dos: &mut Dos, packet: [u8; 3]) {
    let io = dos.cpu().context().io.clone();
    for byte in packet {
        inject(&io, byte).await;
        assert_eq!(
            common::run_until(dos.cpu(), 0x102).await,
            ExecutionExit::Interrupted
        );
    }
}

#[test]
fn irq12_far_callback_observes_ibm_stack_and_preserves_interrupted_registers() {
    with_mouse(async |_, mut dos| {
        install(&mut dos).await;
        let io = dos.cpu().context().io.clone();
        let status = success(&mut dos, 6, 0).await;
        assert_eq!(
            status.registers.ebx as u16, 0x20,
            "status observes enabled reporting"
        );
        let state = dos.cpu().cpu_state();
        state.set_eax(0x12345678);
        state.set_bx(0x4321);
        state.set_si(0x8765);
        state.set_di(0x789a);
        state.set_bp(0xabcd);
        state.set_eflags(0x643);
        state.set_segment(Segment::Ds, SegmentRegister::real_mode(Segment::Ds, 0x2345));
        let before = state.snapshot();
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        // A byte without the header marker does not start a packet.
        inject(&io, 0).await;
        common::run_until(dos.cpu(), 0x102).await;
        for byte in [0x2d, 0xf8] {
            inject(&io, byte).await;
            common::run_until(dos.cpu(), 0x102).await;
            assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), 0);
        }
        inject(&io, 0xfd).await;
        common::run_until(dos.cpu(), 0x102).await;
        let mut arguments = [0; 8];
        memory.view().read(base + 0x500, &mut arguments).unwrap();
        assert_eq!(arguments, [0, 0, 0xfd, 0, 0xf8, 0, 0x2d, 0]);
        assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), 1);
        let after = state.snapshot();
        assert_eq!(after.registers, before.registers);
        assert_eq!(after.segments, before.segments);
        assert_eq!(after.flags.eflags(), before.flags.eflags());
        assert_eq!(after.eip, before.eip);
        deliver(&mut dos, [8, 1, 2]).await;
        assert_eq!(
            memory.view().read_u16(base + 0x508).unwrap(),
            2,
            "both PIC EOIs permit the next packet"
        );
    });
}

#[test]
fn disable_and_callback_removal_drop_partial_packets() {
    with_mouse(async |_, mut dos| {
        install(&mut dos).await;
        let io = dos.cpu().context().io.clone();
        let memory = dos.cpu().memory();
        let base = u32::from(dos.process().unwrap().psp_segment()) << 4;
        dos.cpu().cpu_state().set_interrupts_enabled(true);
        inject(&io, 0x29).await;
        common::run_until(dos.cpu(), 0x102).await;
        success(&mut dos, 0, 0).await;
        success(&mut dos, 0, 0x0100).await;
        dos.cpu().cpu_state().set_interrupts_enabled(true);
        deliver(&mut dos, [8, 2, 3]).await;
        assert_eq!(memory.view().read_u16(base + 0x506).unwrap(), 8);
        assert_eq!(memory.view().read_u16(base + 0x508).unwrap(), 1);
        dos.cpu()
            .cpu_state()
            .set_segment(Segment::Es, SegmentRegister::real_mode(Segment::Es, 0));
        success(&mut dos, 7, 0).await;
        let state = super::call(&mut dos, 0xc200, 0x0100).await;
        assert_eq!(state.registers.eax as u16, 0x0500);
        assert_eq!(state.flags.eflags() & 1, 1);
        let status = success(&mut dos, 6, 0).await;
        assert_eq!(status.registers.ebx as u16, 0);
    });
}
