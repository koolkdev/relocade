use crate::common;
use relocade_x86::ExecutionExit;

#[test]
fn absent_disk_probes_and_status_return_to_the_guest() {
    pollster::block_on(async {
        let mut dos = common::dos().await;
        dos.launch_com(&[0xcd, 0x13, 0xeb, 0xfe], Default::default())
            .unwrap();
        let memory = dos.cpu().memory();
        memory.view().write(0x10300, &[0xa5; 512]).unwrap();
        // Independent floppy and hard-disk status bytes; no drive is installed.
        for (drive, status_address, other_status) in [(0, 0x441, 0x474), (0x80, 0x474, 0x441)] {
            memory.view().write_u8(other_status, 0x20).unwrap();
            for (function, status, carry) in [
                (0x08, 7, true),
                (0x01, 7, true), // Previous failed parameter query.
                (0x15, 0, false),
                (0x01, 0, false),
                (0x02, 1, true), // Read cannot touch the destination.
                (0x03, 1, true), // Write cannot report success.
                (0x41, 1, true), // EDD installation check must not advertise support.
                (0x01, 1, true),
            ] {
                let state = dos.cpu().cpu_state();
                state.set_eip(0x100);
                state.set_ax((function << 8) | 1);
                state.set_bx(if function == 0x41 { 0x55aa } else { 0x300 });
                state.set_cx(1);
                state.set_dx(0x1200 | drive);
                state.set_eflags(0xa56);
                let before = state.snapshot();
                assert_eq!(
                    common::run_until(dos.cpu(), 0x102).await,
                    ExecutionExit::Interrupted
                );
                assert_eq!(state.ax(), (u16::from(status) << 8) | 1);
                assert_eq!(state.bx(), before.registers.ebx as u16);
                assert_eq!(state.cx(), 1);
                assert_eq!(
                    state.dx(),
                    if function == 0x08 {
                        0x1200
                    } else {
                        0x1200 | drive
                    }
                );
                assert_eq!(state.sp(), before.registers.esp as u16);
                assert_eq!(state.eflags(), 0xa56 | u32::from(carry));
                assert_eq!(memory.view().read_u8(status_address).unwrap(), status);
                assert_eq!(memory.view().read_u8(other_status).unwrap(), 0x20);
            }
        }
        let mut buffer = [0; 512];
        memory.view().read(0x10300, &mut buffer).unwrap();
        assert_eq!(buffer, [0xa5; 512]);
    });
}
